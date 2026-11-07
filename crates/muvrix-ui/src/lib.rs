//! `muvrix-ui`: Desktop UI and OpenGL rendering pipeline for Muvrix.

pub mod app;
pub mod fbo;
pub mod gl_loader;
pub mod icons;
pub mod quad;
pub mod render_engine;
pub mod views;

pub use app::MuvrixApp;
pub use views::AppView;
