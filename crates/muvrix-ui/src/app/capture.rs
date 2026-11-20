//! Environment-gated UI capture harness.
//!
//! There is no reliable, permission-free way to screenshot a Wayland window
//! from outside the app, so the app asks the runner to read back its own
//! composited framebuffer and write a PNG. It is inert unless
//! `MUVRIX_CAPTURE_DIR` is set, and only exists so the player HUD states can be
//! inspected visually during development.

use eframe::egui;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::MuvrixApp;

const PHASES: [(u64, &str); 9] = [
    (3_000, "01-windowed-hud"),
    (4_400, "02-windowed-playlist"),
    (5_800, "03-stats-overlay"),
    (7_600, "04-cinema-hud"),
    (9_400, "05-cinema-hidden"),
    (10_800, "06-windowed-restored"),
    // Same deck at the two extremes the window can be dragged to.
    (12_600, "07-min-window"),
    (14_200, "08-max-window"),
    // Late sample: separates startup/seek drops from steady-state drops.
    (22_000, "09-late-stats"),
];

#[derive(Default)]
pub(crate) struct CaptureState {
    pub(crate) dir: Option<PathBuf>,
    pub(crate) started: Option<Instant>,
    pub(crate) phase: usize,
    /// Name of the PNG the runner should read back after this frame.
    pub(crate) pending: Option<String>,
    /// When the current phase's state was applied; the readback waits a couple
    /// of frames so the capture shows the new state, not the previous one.
    pub(crate) armed_at: Option<Instant>,
    pub(crate) next_name: Option<String>,
}

impl CaptureState {
    pub(crate) fn from_env() -> Self {
        let dir = std::env::var_os("MUVRIX_CAPTURE_DIR").map(PathBuf::from);
        eprintln!("capture: harness dir = {dir:?}");
        Self {
            dir,
            ..Default::default()
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.dir.is_some()
    }
}

impl MuvrixApp {
    /// Advances the scripted capture timeline. State for a phase is applied
    /// first and the readback is requested for the frame that renders it.
    pub(crate) fn drive_capture(&mut self, ctx: &egui::Context) {
        if !self.capture.enabled() {
            return;
        }
        let started = *self.capture.started.get_or_insert_with(Instant::now);
        let elapsed = started.elapsed().as_millis() as u64;

        let Some((due, name)) = PHASES.get(self.capture.phase).copied() else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        };
        // A freshly applied state needs a rendered frame before it is read
        // back; egui window/area geometry settles one frame late.
        if let Some(armed) = self.capture.armed_at {
            if armed.elapsed() < Duration::from_millis(220) {
                ctx.request_repaint_after(Duration::from_millis(50));
                return;
            }
            self.capture.armed_at = None;
            if let Some(name) = self.capture.next_name.take() {
                eprintln!("capture: requesting {name}");
                self.capture.pending = Some(name);
            }
            ctx.request_repaint();
            return;
        }

        if elapsed < due {
            ctx.request_repaint_after(Duration::from_millis(60));
            return;
        }

        match self.capture.phase {
            0 => {
                self.show_playlist_drawer = false;
                self.show_playback_stats = false;
                self.sync_panel_open = false;
                self.osd_manually_hidden = false;
                self.overlay_alpha = 1.0;
            }
            1 => {
                self.show_playlist_drawer = true;
            }
            2 => {
                self.show_playlist_drawer = false;
                self.show_playback_stats = true;
            }
            3 => {
                self.show_playback_stats = false;
                if !self.is_cinema_mode {
                    self.toggle_cinema(ctx);
                }
                self.osd_manually_hidden = false;
                self.overlay_alpha = 1.0;
            }
            4 => {
                self.osd_manually_hidden = true;
                self.overlay_alpha = 0.0;
                self.ui_overlay_visible = false;
            }
            5 => {
                self.osd_manually_hidden = false;
                self.overlay_alpha = 1.0;
                if self.is_cinema_mode || self.is_fullscreen {
                    self.exit_fullscreen(ctx);
                }
            }
            6 => {
                if self.is_cinema_mode || self.is_fullscreen {
                    self.exit_fullscreen(ctx);
                }
                self.show_playlist_drawer = false;
                self.show_playback_stats = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(660.0, 420.0)));
            }
            7 => {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1600.0, 940.0)));
            }
            8 => {
                self.show_playback_stats = true;
            }
            _ => {}
        }

        self.capture.armed_at = Some(Instant::now());
        self.capture.next_name = Some(name.to_string());
        self.capture.phase += 1;
        ctx.request_repaint();
    }

    /// Takes the pending readback request for the runner to service after it
    /// has composited the frame.
    pub(crate) fn take_pending_capture(&mut self) -> Option<(PathBuf, String)> {
        let name = self.capture.pending.take()?;
        let dir = self.capture.dir.clone()?;
        Some((dir, name))
    }

    /// Writes a readback buffer (already flipped to top-down RGBA) to disk.
    pub(crate) fn save_capture(
        &self,
        dir: &std::path::Path,
        name: &str,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) {
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join(format!("{name}.png"));
        match image::save_buffer(
            &path,
            pixels,
            width,
            height,
            image::ExtendedColorType::Rgba8,
        ) {
            Ok(()) => eprintln!("capture: wrote {}", path.display()),
            Err(err) => eprintln!("capture: write failed: {err}"),
        }
    }
}
