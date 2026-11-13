//! Renderer modules owned by Muvrix instead of by the windowing framework.
//!
//! * [`egl`] — raw EGL binding.
//! * [`presenter`] — surface, swap interval, presentation state.
//! * [`color`] — `wp_color_manager_v1` image descriptions.
//! * [`precision`] — proof of the surface's actual quantisation.
//! * [`composite`] — video + UI composition in the correct domain.

pub mod color;
pub mod composite;
pub mod egl;
pub mod precision;
pub mod presenter;

pub use composite::{ColorMode, Composite};
pub use precision::PrecisionReport;
pub use presenter::{PresentationState, Presenter, SurfaceFormat, SwapStats};
