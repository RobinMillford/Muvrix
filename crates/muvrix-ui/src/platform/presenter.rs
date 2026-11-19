//! EGL presentation surface owned by Muvrix.
//!
//! Responsibilities, deliberately narrow:
//!
//! * select the EGL configuration (10-bit when the driver/compositor allow it),
//! * create the GLES context and the window surface on winit's `wl_surface`
//!   (Wayland) or on winit's X11 window (X11),
//! * own the swap interval and the swap call,
//! * carry an explicit presentation state machine and its timing.
//!
//! It does **not** own the Wayland event loop (winit does) and does not read the
//! socket. That keeps exactly one reader on the connection, which is what the
//! Phase 1.9 investigation identified as the critical invariant.

use std::ffi::c_void;
use std::os::raw::c_int;
use std::time::Instant;

use crate::platform::egl;

/// Explicit presentation state. `AwaitingFeedback` exists so that "swapped" and
/// "the compositor consumed it" are never conflated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationState {
    Idle,
    Ready,
    Submitted,
    AwaitingFeedback,
    Presented,
    Unavailable,
    Recovering,
    DeviceLost,
    Failed,
}

impl PresentationState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Ready => "ready",
            Self::Submitted => "submitted",
            Self::AwaitingFeedback => "awaiting-feedback",
            Self::Presented => "presented",
            Self::Unavailable => "unavailable",
            Self::Recovering => "recovering",
            Self::DeviceLost => "device-lost",
            Self::Failed => "failed",
        }
    }
}

/// Which native windowing system the surface is created on.
///
/// The EGL configuration, context and swap path are identical for both; only
/// platform selection and the native window object differ. Keeping them in one
/// type is deliberate: a second present-on-X11 path would be a second set of
/// swap/timing semantics, which is exactly what the presentation state machine
/// exists to avoid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceTarget {
    Wayland,
    X11,
}

impl SurfaceTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wayland => "wayland",
            Self::X11 => "x11",
        }
    }
}

/// Surface format actually selected, with its provenance.
#[derive(Debug, Clone, Copy, Default)]
pub struct SurfaceFormat {
    pub red: i32,
    pub green: i32,
    pub blue: i32,
    pub alpha: i32,
    pub config_id: i32,
    /// `true` when the chosen config has >= 10 bits per colour channel.
    pub ten_bit: bool,
    /// `true` when a 10-bit config was requested but only an 8-bit one exists.
    pub degraded: bool,
    pub samples: i32,
}

#[derive(Debug, Clone, Default)]
pub struct SwapStats {
    pub swaps: u64,
    pub swap_failures: u64,
    pub last_swap_us: u64,
    pub swap_us_total: u64,
    pub max_swap_us: u64,
    pub slow_swaps_over_50ms: u64,
    pub longest_gap_us: u64,
    last_swap_at: Option<Instant>,
    pub unavailable_episodes: u64,
    pub unavailable_ms_total: u64,
    pub longest_unavailable_ms: u64,
}

impl SwapStats {
    pub fn avg_swap_us(&self) -> u64 {
        self.swap_us_total.checked_div(self.swaps).unwrap_or(0)
    }
}

/// Owned EGL display/config/context/surface triple.
pub struct Presenter {
    display: egl::EGLDisplay,
    config: egl::EGLConfig,
    context: egl::EGLContext,
    surface: egl::EGLSurface,
    egl_window: egl::WlEglWindow,
    format: SurfaceFormat,
    swap_interval: i32,
    interval_min: i32,
    interval_max: i32,
    state: PresentationState,
    stats: SwapStats,
    width: u32,
    height: u32,
    display_ptr: *mut c_void,
    surface_ptr: *mut c_void,
    api_bound: &'static str,
    unavailable_started: Option<Instant>,
    target: SurfaceTarget,
    /// `true` when this presenter opened the X11 `Display*` itself and must
    /// close it on destroy. Wayland's connection always belongs to winit.
    owns_display: bool,
}

impl std::fmt::Debug for Presenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Presenter")
            .field("format", &self.format)
            .field("swap_interval", &self.swap_interval)
            .field("state", &self.state)
            .field("target", &self.target)
            .field("size", &(self.width, self.height))
            .finish()
    }
}

impl Presenter {
    /// Creates the EGL surface on `surface_ptr` (winit's `wl_surface`).
    ///
    /// # Safety
    ///
    /// `display_ptr` and `surface_ptr` must be the live Wayland connection and
    /// surface owned by winit, and must outlive the returned presenter.
    pub unsafe fn new(
        display_ptr: *mut c_void,
        surface_ptr: *mut c_void,
        width: u32,
        height: u32,
        want_ten_bit: bool,
    ) -> Result<Self, String> {
        Self::new_for_target(
            SurfaceTarget::Wayland,
            display_ptr,
            surface_ptr,
            width,
            height,
            want_ten_bit,
        )
    }

    /// Creates the EGL surface on an X11 window.
    ///
    /// `window` is winit's `xcb_window_t`. The Xlib connection is opened here
    /// (see `egl::XOpenDisplay` for why it cannot be borrowed from winit) and is
    /// owned by the returned presenter.
    ///
    /// # Safety
    ///
    /// `window` must be a live X11 window that outlives the returned presenter,
    /// and this must run on the thread that keeps the GL context current.
    pub unsafe fn new_x11_window(
        window: u64,
        width: u32,
        height: u32,
        want_ten_bit: bool,
    ) -> Result<Self, String> {
        if window == 0 {
            return Err("null X11 window".into());
        }
        let display_ptr = egl::XOpenDisplay(std::ptr::null());
        if display_ptr.is_null() {
            return Err("XOpenDisplay(NULL) failed: no X server reachable".into());
        }
        match Self::new_for_target(
            SurfaceTarget::X11,
            display_ptr,
            window as usize as *mut c_void,
            width,
            height,
            want_ten_bit,
        ) {
            Ok(presenter) => Ok(presenter),
            Err(e) => {
                egl::XCloseDisplay(display_ptr);
                Err(e)
            }
        }
    }

    /// Shared construction path for both native targets.
    ///
    /// # Safety
    ///
    /// `display_ptr` must be the live connection for `target` and
    /// `surface_ptr` the live `wl_surface` (Wayland) or window XID (X11); both
    /// must outlive the returned presenter.
    unsafe fn new_for_target(
        target: SurfaceTarget,
        display_ptr: *mut c_void,
        surface_ptr: *mut c_void,
        width: u32,
        height: u32,
        want_ten_bit: bool,
    ) -> Result<Self, String> {
        let plat_name = match target {
            SurfaceTarget::Wayland => "Wayland",
            SurfaceTarget::X11 => "X11",
        };
        if display_ptr.is_null() {
            return Err(format!("null {plat_name} display"));
        }
        if surface_ptr.is_null() {
            return Err(match target {
                SurfaceTarget::Wayland => "null Wayland surface".to_string(),
                SurfaceTarget::X11 => "null X11 window".to_string(),
            });
        }

        let platform = match target {
            SurfaceTarget::Wayland => egl::EGL_PLATFORM_WAYLAND_KHR,
            SurfaceTarget::X11 => egl::EGL_PLATFORM_X11_KHR,
        };
        let display = egl::eglGetPlatformDisplay(platform, display_ptr, std::ptr::null());
        if display.is_null() {
            return Err(format!(
                "eglGetPlatformDisplay({plat_name}) returned no display"
            ));
        }

        let mut major = 0;
        let mut minor = 0;
        if egl::eglInitialize(display, &mut major, &mut minor) == egl::EGL_FALSE {
            let err = egl::eglGetError();
            return Err(format!(
                "eglInitialize failed: {} (0x{err:x})",
                egl::error_name(err)
            ));
        }
        let version = unsafe {
            let s = egl::eglQueryString(display, 0x3054); // EGL_VERSION
            if s.is_null() {
                "unknown".to_string()
            } else {
                std::ffi::CStr::from_ptr(s).to_string_lossy().into_owned()
            }
        };
        log::info!(
            "EGL {major}.{minor} ({version}) on {} display {display_ptr:p}",
            target.as_str()
        );

        if egl::eglBindAPI(egl::EGL_OPENGL_ES_API) == egl::EGL_FALSE {
            let err = egl::eglGetError();
            return Err(format!(
                "eglBindAPI(OpenGL ES) failed: {}",
                egl::error_name(err)
            ));
        }

        // Request 10-bit first. The Phase 1.9 surface POC established that this
        // stack exposes R10G10B10A2 for ES2 renderables, and that an 8-bit
        // surface is what silently breaks PQ output, so the fallback is explicit.
        let attempts: [(i32, i32, i32, i32); 2] = [(10, 10, 10, 2), (8, 8, 8, 8)];
        let mut chosen: Option<(egl::EGLConfig, SurfaceFormat)> = None;
        let mut any_config = false;

        for (i, (r, g, b, a)) in attempts.iter().enumerate() {
            if i == 0 && !want_ten_bit {
                continue;
            }
            let attrs: [egl::EGLint; 17] = [
                egl::EGL_SURFACE_TYPE,
                egl::EGL_WINDOW_BIT,
                egl::EGL_RENDERABLE_TYPE,
                egl::EGL_OPENGL_ES2_BIT,
                egl::EGL_RED_SIZE,
                *r,
                egl::EGL_GREEN_SIZE,
                *g,
                egl::EGL_BLUE_SIZE,
                *b,
                egl::EGL_ALPHA_SIZE,
                *a,
                egl::EGL_DEPTH_SIZE,
                0,
                egl::EGL_STENCIL_SIZE,
                0,
                egl::EGL_NONE,
            ];
            let mut config: egl::EGLConfig = std::ptr::null_mut();
            let mut count: egl::EGLint = 0;
            let ok = egl::eglChooseConfig(display, attrs.as_ptr(), &mut config, 1, &mut count);
            if ok == egl::EGL_FALSE || count == 0 {
                continue;
            }
            any_config = true;
            let mut fmt = SurfaceFormat::default();
            for (attr, slot) in [
                (egl::EGL_RED_SIZE, 0usize),
                (egl::EGL_GREEN_SIZE, 1),
                (egl::EGL_BLUE_SIZE, 2),
                (egl::EGL_ALPHA_SIZE, 3),
                (egl::EGL_CONFIG_ID, 4),
                (egl::EGL_SAMPLES, 5),
            ] {
                let mut v: egl::EGLint = 0;
                let got = egl::eglGetConfigAttrib(display, config, attr, &mut v);
                if got == egl::EGL_FALSE {
                    v = -1;
                }
                match slot {
                    0 => fmt.red = v,
                    1 => fmt.green = v,
                    2 => fmt.blue = v,
                    3 => fmt.alpha = v,
                    4 => fmt.config_id = v,
                    _ => fmt.samples = v,
                }
            }
            fmt.ten_bit = fmt.red >= 10 && fmt.green >= 10 && fmt.blue >= 10;
            if fmt.ten_bit || i == attempts.len() - 1 {
                chosen = Some((config, fmt));
                break;
            }
            log::warn!(
                "no 10-bit EGLConfig for ES2 renderables (got R{} G{} B{} A{}); falling back to 8-bit",
                fmt.red,
                fmt.green,
                fmt.blue,
                fmt.alpha
            );
        }

        let (config, mut format) = chosen.ok_or_else(|| {
            format!(
                "no usable EGLConfig (any={any_config}); EGL error {}",
                egl::error_name(unsafe { egl::eglGetError() })
            )
        })?;

        if want_ten_bit && !format.ten_bit {
            format.degraded = true;
            log::error!(
                "surface format degraded to 8-bit (R{} G{} B{} A{}); HDR output will be refused",
                format.red,
                format.green,
                format.blue,
                format.alpha
            );
        }

        // ES 3.0 where available (needed for the precision probe's float
        // readback and for egui's ES300 shader path); ES 2 as a last resort.
        let mut context = std::ptr::null_mut();
        for client_version in [3, 2] {
            let ctx_attrs = [
                egl::EGL_CONTEXT_CLIENT_VERSION,
                client_version,
                egl::EGL_NONE,
            ];
            context =
                egl::eglCreateContext(display, config, std::ptr::null_mut(), ctx_attrs.as_ptr());
            if !context.is_null() {
                log::info!("EGL context created (OpenGL ES {client_version})");
                break;
            }
        }
        if context.is_null() {
            let err = egl::eglGetError();
            return Err(format!("eglCreateContext failed: {}", egl::error_name(err)));
        }

        // Wayland needs the wl_egl_window wrapper EGL resizes through; X11 takes
        // the window's XID directly, so there is no wrapper to create or destroy.
        let egl_window = match target {
            SurfaceTarget::Wayland => {
                let w = egl::wl_egl_window_create(surface_ptr, width as c_int, height as c_int);
                if w.is_null() {
                    egl::eglDestroyContext(display, context);
                    return Err("wl_egl_window_create failed".into());
                }
                w
            }
            SurfaceTarget::X11 => std::ptr::null_mut(),
        };
        let native_window: egl::EGLNativeWindowType = match target {
            SurfaceTarget::Wayland => egl_window as egl::EGLNativeWindowType,
            SurfaceTarget::X11 => surface_ptr,
        };

        let surface = egl::eglCreateWindowSurface(display, config, native_window, std::ptr::null());
        if surface.is_null() {
            let err = egl::eglGetError();
            if !egl_window.is_null() {
                egl::wl_egl_window_destroy(egl_window);
            }
            egl::eglDestroyContext(display, context);
            return Err(format!(
                "eglCreateWindowSurface failed on {plat_name}: {}",
                egl::error_name(err)
            ));
        }

        if egl::eglMakeCurrent(display, surface, surface, context) == egl::EGL_FALSE {
            let err = egl::eglGetError();
            egl::eglDestroySurface(display, surface);
            if !egl_window.is_null() {
                egl::wl_egl_window_destroy(egl_window);
            }
            egl::eglDestroyContext(display, context);
            return Err(format!("eglMakeCurrent failed: {}", egl::error_name(err)));
        }

        let mut interval_min = 0;
        let mut interval_max = 1;
        egl::eglGetConfigAttrib(
            display,
            config,
            egl::EGL_MIN_SWAP_INTERVAL,
            &mut interval_min,
        );
        egl::eglGetConfigAttrib(
            display,
            config,
            egl::EGL_MAX_SWAP_INTERVAL,
            &mut interval_max,
        );

        let mut presenter = Self {
            display,
            config,
            context,
            surface,
            egl_window,
            format,
            swap_interval: 0,
            interval_min,
            interval_max,
            state: PresentationState::Idle,
            stats: SwapStats::default(),
            width,
            height,
            display_ptr,
            surface_ptr,
            api_bound: "opengl-es",
            unavailable_started: None,
            target,
            owns_display: target == SurfaceTarget::X11,
        };

        // Interval 0 is the fix on Wayland: Mesa's Wayland WSI blocks inside
        // eglSwapBuffers waiting for a frame callback at interval 1. X11 has no
        // such WSI behaviour, so it keeps the normal vsync interval; running X11
        // unthrottled would simply burn a core.
        // Phase 2.11 controlled experiment: test-only interval override.
        let default_interval = match target {
            SurfaceTarget::Wayland => 0,
            SurfaceTarget::X11 => 1,
        };
        let requested_interval = std::env::var("MUVRIX_TEST_EGL_SWAP_INTERVAL")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(default_interval);
        presenter.set_swap_interval(requested_interval)?;
        log::info!(
            "swap interval requested={} applied={} range={}..={}",
            requested_interval,
            presenter.swap_interval,
            presenter.interval_min,
            presenter.interval_max
        );

        log::info!(
            "surface: R{}G{}B{}A{} config_id={} samples={} ten_bit={} interval={} (range {}..={})",
            presenter.format.red,
            presenter.format.green,
            presenter.format.blue,
            presenter.format.alpha,
            presenter.format.config_id,
            presenter.format.samples,
            presenter.format.ten_bit,
            presenter.swap_interval,
            presenter.interval_min,
            presenter.interval_max
        );
        presenter.state = PresentationState::Ready;

        Ok(presenter)
    }

    pub fn format(&self) -> SurfaceFormat {
        self.format
    }

    pub fn state(&self) -> PresentationState {
        self.state
    }

    pub fn stats(&self) -> &SwapStats {
        &self.stats
    }

    pub fn swap_interval(&self) -> i32 {
        self.swap_interval
    }

    pub fn interval_range(&self) -> (i32, i32) {
        (self.interval_min, self.interval_max)
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn display_ptr(&self) -> *mut c_void {
        self.display_ptr
    }

    pub fn surface_ptr(&self) -> *mut c_void {
        self.surface_ptr
    }

    pub fn raw_display(&self) -> egl::EGLDisplay {
        self.display
    }

    pub fn api_bound(&self) -> &'static str {
        self.api_bound
    }

    pub fn target(&self) -> SurfaceTarget {
        self.target
    }

    pub fn set_swap_interval(&mut self, interval: i32) -> Result<(), String> {
        if interval < self.interval_min || interval > self.interval_max {
            log::warn!(
                "swap interval {interval} outside supported range {}..={}; clamping",
                self.interval_min,
                self.interval_max
            );
        }
        let clamped = interval.clamp(self.interval_min, self.interval_max);
        if unsafe { egl::eglSwapInterval(self.display, clamped) } == egl::EGL_FALSE {
            let err = unsafe { egl::eglGetError() };
            return Err(format!(
                "eglSwapInterval({clamped}) failed: {}",
                egl::error_name(err)
            ));
        }
        self.swap_interval = clamped;
        Ok(())
    }

    pub fn make_current(&self) -> Result<(), String> {
        if unsafe { egl::eglMakeCurrent(self.display, self.surface, self.surface, self.context) }
            == egl::EGL_FALSE
        {
            let err = unsafe { egl::eglGetError() };
            return Err(format!("eglMakeCurrent: {}", egl::error_name(err)));
        }
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if width == self.width && height == self.height {
            return;
        }
        // An X11 EGL surface tracks its drawable's size on the server; only the
        // Wayland wl_egl_window carries a client-side size that needs updating.
        if self.target == SurfaceTarget::Wayland {
            unsafe {
                egl::wl_egl_window_resize(self.egl_window, width as c_int, height as c_int, 0, 0);
            }
        }
        self.width = width;
        self.height = height;
        log::debug!("surface resized to {width}x{height}");
    }

    /// Presents the current back buffer and records the timing.
    ///
    /// Returns `false` if the swap reported failure (the caller decides whether
    /// that is a lost surface).
    pub fn swap(&mut self) -> bool {
        let started = Instant::now();
        if let Some(prev) = self.stats.last_swap_at {
            let gap = started.duration_since(prev).as_micros() as u64;
            if gap > self.stats.longest_gap_us {
                self.stats.longest_gap_us = gap;
            }
        }

        let ok = unsafe { egl::eglSwapBuffers(self.display, self.surface) } == egl::EGL_TRUE;
        let elapsed = started.elapsed().as_micros() as u64;

        self.stats.last_swap_at = Some(started);
        self.stats.last_swap_us = elapsed;
        self.stats.swaps += 1;
        self.stats.swap_us_total += elapsed;
        self.stats.max_swap_us = self.stats.max_swap_us.max(elapsed);
        if elapsed > 50_000 {
            self.stats.slow_swaps_over_50ms += 1;
        }

        if ok {
            // Submitted != presented: the compositor consumes it later. The
            // runner promotes this to Presented when winit delivers the next
            // frame callback for the same surface.
            self.state = PresentationState::AwaitingFeedback;
        } else {
            let err = unsafe { egl::eglGetError() };
            self.stats.swap_failures += 1;
            if err == 0x300E {
                self.state = PresentationState::DeviceLost;
            } else {
                self.state = PresentationState::Failed;
            }
            log::warn!("eglSwapBuffers failed: {}", egl::error_name(err));
        }
        ok
    }

    /// Marks presentation as unavailable (compositor withholding callbacks) and
    /// accounts for the outage length.
    pub fn mark_unavailable(&mut self, since: Instant) {
        if self.state != PresentationState::Unavailable {
            self.state = PresentationState::Unavailable;
            self.stats.unavailable_episodes += 1;
            self.unavailable_started = Some(since);
        }
    }

    pub fn mark_presented(&mut self) {
        self.state = PresentationState::Presented;
        if let Some(started) = self.unavailable_started.take() {
            let ms = started.elapsed().as_millis() as u64;
            self.stats.unavailable_ms_total += ms;
            self.stats.longest_unavailable_ms = self.stats.longest_unavailable_ms.max(ms);
            log::info!("presentation resumed after {ms} ms unavailable");
        }
    }

    pub fn mark_ready(&mut self) {
        if matches!(
            self.state,
            PresentationState::Idle
                | PresentationState::Recovering
                | PresentationState::Unavailable
        ) {
            self.state = PresentationState::Ready;
        }
    }

    /// Destroys EGL objects in dependency order. Must run on the rendering
    /// thread with the connection still alive.
    pub fn destroy(&mut self) {
        unsafe {
            egl::eglMakeCurrent(
                self.display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if !self.surface.is_null() {
                egl::eglDestroySurface(self.display, self.surface);
                self.surface = std::ptr::null_mut();
            }
            if !self.egl_window.is_null() {
                egl::wl_egl_window_destroy(self.egl_window);
                self.egl_window = std::ptr::null_mut();
            }
            if !self.context.is_null() {
                egl::eglDestroyContext(self.display, self.context);
                self.context = std::ptr::null_mut();
            }
            egl::eglReleaseThread();
            if self.owns_display && !self.display_ptr.is_null() {
                // We opened this Xlib connection ourselves.
                egl::XCloseDisplay(self.display_ptr);
                self.display_ptr = std::ptr::null_mut();
                self.owns_display = false;
            }
            if !self.display.is_null() {
                // A Wayland connection belongs to winit, so never terminate it.
                self.display = std::ptr::null_mut();
            }
        }
        let _ = self.config;
        self.state = PresentationState::Idle;
    }
}

/// Resolves GL entry points for `glow` and for libmpv's render API.
///
/// `eglGetProcAddress` covers both core and extension functions on Mesa's
/// Wayland EGL, including the ES2 core subset; no second library handle is
/// needed.
pub fn get_proc_address(name: &str) -> *mut c_void {
    let Ok(cname) = std::ffi::CString::new(name) else {
        return std::ptr::null_mut();
    };
    unsafe { egl::eglGetProcAddress(cname.as_ptr()) }
}

/// `MPV_RENDER_PARAM_OPENGL_INIT_PARAMS` callback, kept as a plain `extern "C"`
/// function so the pointer stays valid for the render context's lifetime.
///
/// # Safety
///
/// `name` must be null or a valid NUL-terminated C string. The returned pointer
/// is an EGL/GL entry point and is only valid to call in a context where that
/// function is supported.
pub unsafe extern "C" fn mpv_get_proc_address(
    _ctx: *mut c_void,
    name: *const std::os::raw::c_char,
) -> *mut c_void {
    if name.is_null() {
        return std::ptr::null_mut();
    }
    egl::eglGetProcAddress(name)
}
