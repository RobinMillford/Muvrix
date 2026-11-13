//! Extraction of the native display handle the engine needs for direct
//! hardware-surface import.
//!
//! The handle must belong to the *same* windowing-system connection that owns
//! the application's GL context and surface. eframe stores the real handles it
//! received from winit and exposes them through `HasDisplayHandle`, so no second
//! connection is created here.

use muvrix_player::NativeDisplay;

/// Returns the native display backing `cc`'s window, if this platform/path can
/// provide one that mpv accepts.
///
/// * Wayland: the `wl_display*` of the live connection.
/// * X11 (Xlib): the `Display*` of the live connection.
/// * X11 (XCB): **none** — mpv's `MPV_RENDER_PARAM_X11_DISPLAY` takes an Xlib
///   `Display*`, and winit's X11 backend is XCB-based. We deliberately do not
///   open an unrelated Xlib connection, because the engine requires the display
///   that owns the window. On such a session the engine keeps its copy-back
///   path.
/// * Windows/macOS: not implemented here (see the phase-1.8 platform designs).
pub fn from_creation_context(cc: &eframe::CreationContext<'_>) -> Option<NativeDisplay> {
    use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};

    let handle = match cc.display_handle() {
        Ok(handle) => handle,
        Err(e) => {
            log::debug!("No native display handle available: {e}");
            return None;
        }
    };

    match handle.as_raw() {
        #[cfg(target_os = "linux")]
        RawDisplayHandle::Wayland(w) => {
            let ptr = w.display.as_ptr();
            log::info!("Native display: Wayland wl_display {ptr:p}");
            Some(NativeDisplay::Wayland(ptr))
        }
        #[cfg(target_os = "linux")]
        RawDisplayHandle::Xlib(x) => match x.display {
            Some(display) => {
                let ptr = display.as_ptr();
                log::info!("Native display: X11 Display {ptr:p}");
                Some(NativeDisplay::X11(ptr))
            }
            None => {
                log::warn!("X11 display handle reported no Display*; direct hw decode disabled");
                None
            }
        },
        #[cfg(target_os = "linux")]
        RawDisplayHandle::Xcb(x) => {
            // mpv needs an Xlib Display*; an XCB connection pointer is not
            // interchangeable and inventing a second Xlib connection would
            // violate the "same connection that owns the window" requirement.
            log::info!(
                "Native display: XCB connection {:p} — no Xlib Display* available, \
                 direct hw decode disabled for this session",
                x.connection
                    .map(|c| c.as_ptr())
                    .unwrap_or(std::ptr::null_mut())
            );
            None
        }
        other => {
            log::debug!("Unsupported native display handle: {other:?}");
            None
        }
    }
}
