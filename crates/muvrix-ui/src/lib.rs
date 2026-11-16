//! `muvrix-ui`: Desktop UI and OpenGL rendering pipeline for Muvrix.

pub mod app;
pub mod branding;
pub mod causal_trace;
pub mod clock;
pub mod config_editor;
pub mod crash;
pub mod fbo;
pub mod gl_loader;
pub mod gpu_timing;
pub mod icons;
pub mod library_enrichment;
pub mod mpv_state;
pub mod native_display;
pub mod os_integration;
pub mod output;
#[cfg(target_os = "linux")]
pub mod platform;
pub mod presentation;
pub mod presentation_ack;
pub mod quad;
pub mod remote_playback;
pub mod render_engine;
#[cfg(target_os = "linux")]
pub mod runner;
pub mod telemetry;
pub mod thumbnail_cache;
pub mod trace;
pub mod ui_components;
pub mod views;
pub mod wake;

pub use app::MuvrixApp;
pub use thumbnail_cache::LruTextureCache;
pub use views::AppView;
