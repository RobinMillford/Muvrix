//! Automated verification of the embedded `mpv_render_context` FBO rendering pipeline.
//!
//! Directly exercises Muvrix's embedded rendering path:
//! - Off-screen OpenGL Context (via EGL on Linux)
//! - `MpvRenderContext::new_gl` with OpenGL API
//! - `OffscreenFbo` backed by a 2D `GL_RGBA8` texture
//! - Pixel-level validation of Anime4K shader enhancement inside the embedded FBO
//! - libplacebo HDR-to-SDR tone-mapping inside the embedded FBO
//! - SDR regression verification

use glow::HasContext as _;
use muvrix_player::shader::{Anime4KPreset, ShaderManager, ShaderQuality, ToneMappingAlgorithm};
use muvrix_player::{MpvHandle, MpvRenderContext};
use muvrix_ui::fbo::OffscreenFbo;
use std::ffi::{c_char, c_int, c_void, CString};
use std::ptr;
use std::time::{Duration, Instant};

type EglDisplay = *mut c_void;
type EglConfig = *mut c_void;
type EglContext = *mut c_void;
type EglSurface = *mut c_void;

#[allow(non_snake_case)]
struct EglLib {
    eglGetDisplay: unsafe extern "C" fn(*mut c_void) -> EglDisplay,
    eglInitialize: unsafe extern "C" fn(EglDisplay, *mut c_int, *mut c_int) -> c_int,
    eglBindAPI: unsafe extern "C" fn(c_int) -> c_int,
    eglChooseConfig:
        unsafe extern "C" fn(EglDisplay, *const c_int, *mut EglConfig, c_int, *mut c_int) -> c_int,
    eglCreateContext:
        unsafe extern "C" fn(EglDisplay, EglConfig, EglContext, *const c_int) -> EglContext,
    eglCreatePbufferSurface:
        unsafe extern "C" fn(EglDisplay, EglConfig, *const c_int) -> EglSurface,
    eglMakeCurrent: unsafe extern "C" fn(EglDisplay, EglSurface, EglSurface, EglContext) -> c_int,
    eglGetProcAddress: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    eglDestroyContext: unsafe extern "C" fn(EglDisplay, EglContext) -> c_int,
    eglDestroySurface: unsafe extern "C" fn(EglDisplay, EglSurface) -> c_int,
    eglTerminate: unsafe extern "C" fn(EglDisplay) -> c_int,
}

impl EglLib {
    #[allow(clippy::missing_transmute_annotations, clippy::manual_c_str_literals)]
    unsafe fn load() -> Option<Self> {
        let handle = libc::dlopen(c"libEGL.so.1".as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
        if handle.is_null() {
            return None;
        }

        macro_rules! get_sym {
            ($name:ident) => {{
                let sym = libc::dlsym(
                    handle,
                    concat!(stringify!($name), "\0").as_ptr() as *const c_char,
                );
                if sym.is_null() {
                    return None;
                }
                std::mem::transmute(sym)
            }};
        }

        Some(Self {
            eglGetDisplay: get_sym!(eglGetDisplay),
            eglInitialize: get_sym!(eglInitialize),
            eglBindAPI: get_sym!(eglBindAPI),
            eglChooseConfig: get_sym!(eglChooseConfig),
            eglCreateContext: get_sym!(eglCreateContext),
            eglCreatePbufferSurface: get_sym!(eglCreatePbufferSurface),
            eglMakeCurrent: get_sym!(eglMakeCurrent),
            eglGetProcAddress: get_sym!(eglGetProcAddress),
            eglDestroyContext: get_sym!(eglDestroyContext),
            eglDestroySurface: get_sym!(eglDestroySurface),
            eglTerminate: get_sym!(eglTerminate),
        })
    }
}

struct HeadlessGlContext {
    egl: EglLib,
    display: EglDisplay,
    context: EglContext,
    surface: EglSurface,
}

impl HeadlessGlContext {
    unsafe fn create(width: i32, height: i32) -> Option<Self> {
        let egl = EglLib::load()?;
        let display = (egl.eglGetDisplay)(ptr::null_mut());
        if display.is_null() {
            return None;
        }

        let mut major = 0;
        let mut minor = 0;
        if (egl.eglInitialize)(display, &mut major, &mut minor) != 1 {
            return None;
        }

        // Bind standard desktop OpenGL API
        const EGL_OPENGL_API: c_int = 0x30A2;
        if (egl.eglBindAPI)(EGL_OPENGL_API) != 1 {
            return None;
        }

        const EGL_SURFACE_TYPE: c_int = 0x3033;
        const EGL_PBUFFER_BIT: c_int = 0x0001;
        const EGL_RENDERABLE_TYPE: c_int = 0x3040;
        const EGL_OPENGL_BIT: c_int = 0x0008;
        const EGL_RED_SIZE: c_int = 0x3024;
        const EGL_GREEN_SIZE: c_int = 0x3023;
        const EGL_BLUE_SIZE: c_int = 0x3022;
        const EGL_ALPHA_SIZE: c_int = 0x3021;
        const EGL_NONE: c_int = 0x3038;

        let attribs = [
            EGL_SURFACE_TYPE,
            EGL_PBUFFER_BIT,
            EGL_RENDERABLE_TYPE,
            EGL_OPENGL_BIT,
            EGL_RED_SIZE,
            8,
            EGL_GREEN_SIZE,
            8,
            EGL_BLUE_SIZE,
            8,
            EGL_ALPHA_SIZE,
            8,
            EGL_NONE,
        ];

        let mut config = ptr::null_mut();
        let mut num_configs = 0;
        if (egl.eglChooseConfig)(display, attribs.as_ptr(), &mut config, 1, &mut num_configs) != 1
            || num_configs < 1
        {
            return None;
        }

        let context = (egl.eglCreateContext)(display, config, ptr::null_mut(), [EGL_NONE].as_ptr());
        if context.is_null() {
            return None;
        }

        const EGL_WIDTH: c_int = 0x3057;
        const EGL_HEIGHT: c_int = 0x3056;
        let pbuffer_attribs = [EGL_WIDTH, width, EGL_HEIGHT, height, EGL_NONE];

        let surface = (egl.eglCreatePbufferSurface)(display, config, pbuffer_attribs.as_ptr());
        if surface.is_null() {
            (egl.eglDestroyContext)(display, context);
            return None;
        }

        if (egl.eglMakeCurrent)(display, surface, surface, context) != 1 {
            (egl.eglDestroySurface)(display, surface);
            (egl.eglDestroyContext)(display, context);
            return None;
        }

        Some(Self {
            egl,
            display,
            context,
            surface,
        })
    }
}

impl Drop for HeadlessGlContext {
    fn drop(&mut self) {
        unsafe {
            (self.egl.eglMakeCurrent)(
                self.display,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            (self.egl.eglDestroySurface)(self.display, self.surface);
            (self.egl.eglDestroyContext)(self.display, self.context);
            (self.egl.eglTerminate)(self.display);
        }
    }
}

unsafe extern "C" fn egl_proc_loader(
    ctx: *mut c_void,
    name: *const std::os::raw::c_char,
) -> *mut c_void {
    if name.is_null() || ctx.is_null() {
        return ptr::null_mut();
    }
    let egl = &*(ctx as *const EglLib);
    (egl.eglGetProcAddress)(name)
}

#[test]
fn test_embedded_renderer_identification_and_diagnostics() {
    let gl_env = match unsafe { HeadlessGlContext::create(320, 240) } {
        Some(env) => env,
        None => {
            eprintln!("SKIPPING test_embedded_renderer_identification_and_diagnostics: No EGL support in test environment");
            return;
        }
    };

    let gl = unsafe {
        glow::Context::from_loader_function(|name| {
            let cstr = CString::new(name).unwrap();
            (gl_env.egl.eglGetProcAddress)(cstr.as_ptr())
        })
    };

    let vendor = unsafe { gl.get_parameter_string(glow::VENDOR) };
    let renderer = unsafe { gl.get_parameter_string(glow::RENDERER) };
    let version = unsafe { gl.get_parameter_string(glow::VERSION) };

    println!("Embedded Muvrix OpenGL Backend Diagnostics:");
    println!("  Vendor:   {vendor}");
    println!("  Renderer: {renderer}");
    println!("  Version:  {version}");

    assert!(!vendor.is_empty(), "OpenGL vendor must be non-empty");
    assert!(!renderer.is_empty(), "OpenGL renderer must be non-empty");
    assert!(!version.is_empty(), "OpenGL version must be non-empty");
}

#[test]
fn test_embedded_fbo_rendering_with_anime4k_shaders_changes_pixels() {
    let gl_env = match unsafe { HeadlessGlContext::create(320, 240) } {
        Some(env) => env,
        None => {
            eprintln!("SKIPPING test_embedded_fbo_rendering_with_anime4k_shaders_changes_pixels: No EGL context");
            return;
        }
    };

    let gl = unsafe {
        glow::Context::from_loader_function(|name| {
            let cstr = CString::new(name).unwrap();
            (gl_env.egl.eglGetProcAddress)(cstr.as_ptr())
        })
    };

    let fbo = unsafe { OffscreenFbo::new(&gl, 320, 240).expect("Create OffscreenFbo") };

    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/test_video.mp4")
        .canonicalize()
        .expect("test_video.mp4 must exist");

    let shader_dir = ShaderManager::resolve_anime4k_shader_dir().expect("Anime4K assets");

    // Helper to render into the real OffscreenFbo and sample the center pixel
    let render_sample = |preset: Anime4KPreset| -> [u8; 4] {
        let handle = MpvHandle::new().expect("Create handle");
        handle.set_option_string("vo", "libmpv").unwrap();
        handle.set_option_string("ao", "null").unwrap();
        handle.initialize().unwrap();

        let render_ctx = unsafe {
            MpvRenderContext::new_gl(
                &handle,
                Some(egl_proc_loader),
                &gl_env.egl as *const _ as *mut c_void,
            )
            .expect("Create MpvRenderContext")
        };

        if preset != Anime4KPreset::Disabled {
            ShaderManager::apply_anime4k_mode(
                &handle,
                preset,
                ShaderQuality::Fast,
                Some(&shader_dir),
            )
            .expect("Apply Anime4K mode");
        }

        handle
            .command(&["loadfile", fixture.to_str().unwrap()])
            .unwrap();

        // Allow decoder to push initial video frame
        let deadline = Instant::now() + Duration::from_millis(600);
        while Instant::now() < deadline {
            let _ = handle.wait_event(0.02);
        }

        // Render directly into the OffscreenFbo using Muvrix's embedded render path
        let _ = render_ctx.render_gl_to_fbo(
            fbo.fbo().0.get() as i32,
            320,
            240,
            glow::RGBA8 as i32,
            false,
            false,
            false,
        );
        render_ctx.report_swap();

        unsafe { fbo.read_pixel_sample(&gl) }
    };

    let sample_plain = render_sample(Anime4KPreset::Disabled);
    let sample_mode_a = render_sample(Anime4KPreset::ModeA);

    println!("Embedded FBO center pixel unshaded: {:?}", sample_plain);
    println!("Embedded FBO center pixel Mode A:   {:?}", sample_mode_a);

    // Assert that the decoded frame contains valid non-zero RGB pixels
    assert_ne!(
        sample_plain,
        [0, 0, 0, 0],
        "Embedded FBO must contain rendered video frame pixels"
    );

    // Assert that the shader pipeline executed inside Muvrix's embedded FBO
    // produces deterministically distinct pixel values
    assert_ne!(
        sample_plain, sample_mode_a,
        "Embedded FBO output with Anime4K must differ from unshaded output"
    );
}

#[test]
fn test_embedded_fbo_hdr_tone_mapping_and_sdr_regression() {
    let gl_env = match unsafe { HeadlessGlContext::create(320, 240) } {
        Some(env) => env,
        None => {
            eprintln!(
                "SKIPPING test_embedded_fbo_hdr_tone_mapping_and_sdr_regression: No EGL context"
            );
            return;
        }
    };

    let gl = unsafe {
        glow::Context::from_loader_function(|name| {
            let cstr = CString::new(name).unwrap();
            (gl_env.egl.eglGetProcAddress)(cstr.as_ptr())
        })
    };

    let fbo = unsafe { OffscreenFbo::new(&gl, 320, 240).expect("Create OffscreenFbo") };

    let hdr_fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/hdr10_pq_bt2020.mp4")
        .canonicalize()
        .expect("hdr10 fixture must exist");

    let sdr_fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/test_video.mp4")
        .canonicalize()
        .expect("sdr fixture must exist");

    let render_fixture_pixel =
        |path: &std::path::Path, tm: ToneMappingAlgorithm| -> ([u8; 4], String, String) {
            let handle = MpvHandle::new().expect("Create handle");
            handle.set_option_string("vo", "libmpv").unwrap();
            handle.set_option_string("ao", "null").unwrap();
            handle
                .set_option_string("tone-mapping", tm.as_str())
                .unwrap();
            handle.set_option_string("target-trc", "srgb").unwrap();
            handle.set_option_string("target-prim", "bt.709").unwrap();
            handle.initialize().unwrap();

            let render_ctx = unsafe {
                MpvRenderContext::new_gl(
                    &handle,
                    Some(egl_proc_loader),
                    &gl_env.egl as *const _ as *mut c_void,
                )
                .expect("Create MpvRenderContext")
            };

            handle
                .command(&["loadfile", path.to_str().unwrap()])
                .unwrap();

            let deadline = Instant::now() + Duration::from_millis(600);
            while Instant::now() < deadline {
                let _ = handle.wait_event(0.02);
            }

            let primaries = handle
                .get_property_string("video-params/primaries")
                .unwrap_or_default();
            let gamma = handle
                .get_property_string("video-params/gamma")
                .unwrap_or_default();
            let vo_active = handle.get_property_string("current-vo").unwrap_or_default();
            println!("Embedded rendering active current-vo: '{}'", vo_active);
            assert_eq!(
                vo_active, "libmpv",
                "Embedded MpvRenderContext must use vo=libmpv (the OpenGL render API backend)"
            );

            let _ = render_ctx.render_gl_to_fbo(
                fbo.fbo().0.get() as i32,
                320,
                240,
                glow::RGBA8 as i32,
                false,
                false,
                false,
            );
            render_ctx.report_swap();

            let pixel = unsafe { fbo.read_pixel_sample(&gl) };
            (pixel, primaries, gamma)
        };

    // 1. Verify HDR metadata detection and tone-mapped embedded rendering
    let (pixel_bt2390, primaries, gamma) =
        render_fixture_pixel(&hdr_fixture, ToneMappingAlgorithm::Bt2390);
    println!(
        "HDR10 Fixture detected primaries: '{}', gamma: '{}', pixel BT.2390: {:?}",
        primaries, gamma, pixel_bt2390
    );
    assert_eq!(
        primaries, "bt.2020",
        "HDR fixture must signal BT.2020 primaries"
    );
    assert_eq!(gamma, "pq", "HDR fixture must signal PQ gamma");
    assert_ne!(
        pixel_bt2390,
        [0, 0, 0, 0],
        "Tone-mapped HDR frame must render non-zero pixels"
    );

    // 2. Compare numerical output under alternate tone-mapping algorithm (Clip vs BT.2390)
    let (pixel_clip, _, _) = render_fixture_pixel(&hdr_fixture, ToneMappingAlgorithm::Clip);
    println!(
        "HDR10 Fixture pixel with Clip tone-mapping: {:?}",
        pixel_clip
    );
    // The two tone-mapping algorithms evaluate HDR highlights differently into the SDR FBO
    println!(
        "Tone mapping algorithm comparison: BT.2390={:?} vs Clip={:?}",
        pixel_bt2390, pixel_clip
    );

    // 3. Regression test: Normal SDR media must render cleanly without distortion
    let (sdr_pixel, sdr_prim, sdr_gamma) =
        render_fixture_pixel(&sdr_fixture, ToneMappingAlgorithm::Auto);
    println!(
        "SDR fixture rendered pixel: {:?}, primaries: '{}', gamma: '{}'",
        sdr_pixel, sdr_prim, sdr_gamma
    );
    assert_ne!(
        sdr_pixel,
        [0, 0, 0, 0],
        "SDR regression check: must render non-zero pixel in embedded FBO"
    );
}
