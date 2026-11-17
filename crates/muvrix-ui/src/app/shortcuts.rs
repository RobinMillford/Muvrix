use std::time::Instant;

use eframe::egui::{self, RichText};
use muvrix_player::PlaybackState;

use crate::app::MuvrixApp;
use crate::views::AppView;

impl MuvrixApp {
    /// Checks `ctx.wants_keyboard_input()` to ensure shortcuts do not trigger
    /// while the user is actively typing in a text field.
    pub(crate) fn handle_keyboard_inputs(&mut self, ctx: &egui::Context) {
        // Do NOT intercept shortcuts if user is typing into an active text field!
        if ctx.wants_keyboard_input() {
            return;
        }

        // Space: Toggle Play/Pause
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.toggle_pause();
        }

        // Left / Right Arrow: Relative Seek (-5s / +5s)
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            let _ = self.controller.seek_relative(-5.0);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            let _ = self.controller.seek_relative(5.0);
        }

        // Up / Down Arrow: Volume adjustments (+5% / -5%)
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            let _ = self.controller.adjust_volume(5.0);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            let _ = self.controller.adjust_volume(-5.0);
        }

        // . / ,: frame-step forward/back
        if ctx.input(|i| i.key_pressed(egui::Key::Period)) {
            self.frame_step();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Comma)) {
            self.frame_back_step();
        }

        // L: cycle A-B loop
        if ctx.input(|i| i.key_pressed(egui::Key::L)) {
            self.cycle_ab_loop();
        }

        // M: Mute / Unmute toggle
        if ctx.input(|i| i.key_pressed(egui::Key::M)) {
            let _ = self.controller.toggle_mute();
        }

        // F or F11: Fullscreen toggle
        if ctx.input(|i| i.key_pressed(egui::Key::F) || i.key_pressed(egui::Key::F11)) {
            self.toggle_fullscreen(ctx);
        }

        // Escape: Exit Fullscreen or dismiss modal / popover
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.player_more_overflow_open {
                self.player_more_overflow_open = false;
            } else if self.player_empty_url_input_open {
                self.player_empty_url_input_open = false;
            } else if self.show_shortcuts {
                self.show_shortcuts = false;
            } else if self.is_fullscreen {
                self.exit_fullscreen(ctx);
            }
        }

        // Alt+M: Toggle Player More (...) Overflow Menu
        if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::M)) {
            self.player_more_overflow_open = !self.player_more_overflow_open;
        }

        // Ctrl+D or Alt+D: Toggle Secondary Playback Controls Deck
        if ctx.input(|i| (i.modifiers.command || i.modifiers.alt) && i.key_pressed(egui::Key::D)) {
            let screen_w = ctx.screen_rect().width();
            if screen_w >= 1300.0 {
                self.player_secondary_deck_collapsed_wide =
                    !self.player_secondary_deck_collapsed_wide;
            } else {
                self.player_secondary_deck_expanded = !self.player_secondary_deck_expanded;
            }
        }

        // Ctrl+U: Toggle URL streaming input in empty state
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::U)) {
            self.player_empty_url_input_open = !self.player_empty_url_input_open;
        }

        // [ and ]: Playback speed decrease / increase (-0.25x / +0.25x)
        if ctx.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
            let _ = self.controller.adjust_speed(-0.25);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
            let _ = self.controller.adjust_speed(0.25);
        }

        // Backspace: Reset speed to 1.0x
        if ctx.input(|i| i.key_pressed(egui::Key::Backspace)) {
            let _ = self.controller.reset_speed();
        }

        // Ctrl+O (or Cmd+O): Open File dialog
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
            self.open_file_dialog();
        }

        // H: Toggle UI overlay / OSD
        if ctx.input(|i| i.key_pressed(egui::Key::H)) {
            self.ui_overlay_visible = !self.ui_overlay_visible;
            self.osd_manually_hidden = !self.ui_overlay_visible;
            self.last_mouse_activity = Instant::now();
        }

        // P: Toggle Playlist drawer
        if ctx.input(|i| i.key_pressed(egui::Key::P)) {
            self.show_playlist_drawer = !self.show_playlist_drawer;
        }

        // B: Toggle Media Browser view
        if ctx.input(|i| i.key_pressed(egui::Key::B)) {
            self.current_view = if self.current_view == AppView::Browser {
                AppView::Player
            } else {
                AppView::Browser
            };
        }

        // I: Toggle Picture-in-Picture mode
        if ctx.input(|i| i.key_pressed(egui::Key::I)) {
            self.toggle_pip(ctx);
        }

        // F1: Toggle shortcuts help dialog
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.show_shortcuts = !self.show_shortcuts;
        }
    }

    /// Automatically hides on-screen playback controls during uninterrupted playback.
    pub(crate) fn update_osd_autohide(&mut self, ctx: &egui::Context) {
        // Any open modals, popovers, or drawers prevent controls from auto-hiding
        let modal_active = self.show_shortcuts
            || self.show_diagnostics
            || self.show_playlist_drawer
            || self.add_root_dialog_open
            || self.pending_resume_prompt.is_some()
            || self.crash_notice.is_some()
            || self.player_more_overflow_open;

        if modal_active {
            self.last_mouse_activity = Instant::now();
            self.ui_overlay_visible = true;
            return;
        }

        // Only auto-hide in the Player view during active, non-paused playback
        if self.current_view != AppView::Player {
            self.ui_overlay_visible = true;
            return;
        }

        // Test-override hooks if present
        if std::env::var("MUVRIX_FORCE_OSD")
            .map(|v| v == "0")
            .unwrap_or(false)
        {
            self.ui_overlay_visible = false;
            return;
        }
        if std::env::var("MUVRIX_FORCE_OSD")
            .map(|v| v == "1")
            .unwrap_or(false)
        {
            self.ui_overlay_visible = true;
            return;
        }

        let is_playing =
            *self.controller.state() == PlaybackState::Playing && !self.controller.is_paused();

        let current_mouse_pos = ctx.input(|i| i.pointer.hover_pos());
        let mouse_moved = match (self.last_mouse_pos, current_mouse_pos) {
            (Some(last), Some(cur)) => last.distance(cur) > 2.0,
            (None, Some(_)) => true,
            _ => false,
        };
        let mouse_active = ctx
            .input(|i| i.pointer.any_down() || i.pointer.any_pressed() || i.pointer.any_released());

        let screen_h = ctx.screen_rect().height();
        let cursor_over_controls = current_mouse_pos
            .is_some_and(|pos| pos.y <= 52.0 || pos.y >= (screen_h - 110.0).max(0.0));

        if mouse_moved || mouse_active {
            self.last_mouse_activity = Instant::now();
            self.last_mouse_pos = current_mouse_pos;
            self.ui_overlay_visible = true;
            self.osd_manually_hidden = false;
        } else if cursor_over_controls && self.ui_overlay_visible {
            // Hovering over controls keeps them visible without timer expiring
            self.last_mouse_activity = Instant::now();
        }

        if is_playing {
            if !self.osd_manually_hidden && !cursor_over_controls {
                let elapsed = self.last_mouse_activity.elapsed();
                if elapsed >= self.osd_autohide_delay {
                    self.ui_overlay_visible = false;
                } else {
                    let remaining = self.osd_autohide_delay.saturating_sub(elapsed);
                    ctx.request_repaint_after(remaining);
                }
            }
        } else if !self.osd_manually_hidden {
            // Paused playback: controls stay visible unless user explicitly toggled Key::H
            self.ui_overlay_visible = true;
        }
    }

    pub(crate) fn render_shortcuts_modal(&mut self, ctx: &egui::Context) {
        // 4. Modal Shortcuts Dialog
        // ---------------------------------------------------------------------
        if self.show_shortcuts {
            let mut open = true;
            let mut close_requested = false;
            egui::Window::new("Keyboard Shortcuts")
                .open(&mut open)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_width(380.0);
                    egui::Grid::new("shortcuts_grid")
                        .striped(true)
                        .spacing([20.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("Space").strong().monospace());
                            ui.label("Play / Pause Toggle");
                            ui.end_row();

                            ui.label(RichText::new("Left / Right").strong().monospace());
                            ui.label("Seek -5s / +5s");
                            ui.end_row();

                            ui.label(RichText::new("Up / Down").strong().monospace());
                            ui.label("Volume +5% / -5%");
                            ui.end_row();

                            ui.label(RichText::new(". / ,").strong().monospace());
                            ui.label("Next / Previous Frame");
                            ui.end_row();

                            ui.label(RichText::new("L").strong().monospace());
                            ui.label("Cycle A-B Loop");
                            ui.end_row();

                            ui.label(RichText::new("M").strong().monospace());
                            ui.label("Mute / Unmute Audio");
                            ui.end_row();

                            ui.label(RichText::new("F / F11").strong().monospace());
                            ui.label("Toggle Fullscreen");
                            ui.end_row();

                            ui.label(RichText::new("Esc").strong().monospace());
                            ui.label("Exit Fullscreen / Dismiss");
                            ui.end_row();

                            ui.label(RichText::new("[ / ]").strong().monospace());
                            ui.label("Speed -0.25x / +0.25x");
                            ui.end_row();

                            ui.label(RichText::new("Backspace").strong().monospace());
                            ui.label("Reset Speed to 1.0x");
                            ui.end_row();

                            ui.label(RichText::new("Ctrl+O").strong().monospace());
                            ui.label("Open Media File");
                            ui.end_row();

                            ui.label(RichText::new("P").strong().monospace());
                            ui.label("Toggle Queue Drawer");
                            ui.end_row();

                            ui.label(RichText::new("B").strong().monospace());
                            ui.label("Toggle Media Browser");
                            ui.end_row();

                            ui.label(RichText::new("I").strong().monospace());
                            ui.label("Toggle Compact Mini-Player");
                            ui.end_row();

                            ui.label(RichText::new("Double Click").strong().monospace());
                            ui.label("Toggle Fullscreen");
                            ui.end_row();

                            ui.label(RichText::new("Single Click").strong().monospace());
                            ui.label("Play / Pause (Video)");
                            ui.end_row();
                        });
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
            self.show_shortcuts = open && !close_requested;
        }
    }
}
