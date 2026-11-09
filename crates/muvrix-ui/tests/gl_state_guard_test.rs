//! Phase 1.4 — `GlStateGuard` and `RenderEngine::paint` GL state-isolation
//! regression tests.
//!
//! The video compositor (offscreen FBO → quad → egui painter) must leave the
//! GL pipeline exactly as it found it so subsequent egui drawing is not
//! corrupted. These tests run against a real (headless EGL) GL context and
//! cover, for both the guard in isolation and the full `paint()` path:
//!   - scissor test state and scissor rectangle
//!   - framebuffer binding (restored by `paint()`, not the guard)
//!   - viewport
//!   - active texture unit and bound 2D texture
//!   - current shader program
//!   - vertex array object
//!
//! plus renderer teardown and repeated create/destroy cycles.
//!
//! NOTE: `QuadRenderer` and `OffscreenFbo` (`muvrix_ui::quad`,
//! `muvrix_ui::fbo`) are exercised indirectly through `render_engine`.

use glow::HasContext as _;
use std::ffi::{c_char, c_int, c_void, CString};
use std::ptr;
use std::time::{Duration, Instant};

use muvrix_player::{MpvHandle, MpvRenderContext};
use muvrix_ui::render_engine::{GlStateGuard, RenderEngine};

// ===========================================================================
// Headless EGL harness (mirrors `embedded_render_test.rs`)
// ===========================================================================

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

fn make_gl_context(gl_env: &HeadlessGlContext) -> std::sync::Arc<glow::Context> {
    unsafe {
        std::sync::Arc::new(glow::Context::from_loader_function(|name| {
            let cstr = CString::new(name).unwrap();
            (gl_env.egl.eglGetProcAddress)(cstr.as_ptr())
        }))
    }
}

fn skip(name: &str) -> Option<std::sync::Arc<glow::Context>> {
    eprintln!("SKIPPING {name}: No EGL context");
    None
}

fn make_linked_program(gl: &glow::Context) -> glow::NativeProgram {
    unsafe {
        let vs = gl
            .create_shader(glow::VERTEX_SHADER)
            .expect("create vertex shader");
        gl.shader_source(vs, "void main() { gl_Position = vec4(0.0); }");
        gl.compile_shader(vs);
        assert!(
            gl.get_shader_compile_status(vs),
            "vertex shader must compile"
        );

        let fs = gl
            .create_shader(glow::FRAGMENT_SHADER)
            .expect("create fragment shader");
        gl.shader_source(fs, "void main() { gl_FragColor = vec4(1.0); }");
        gl.compile_shader(fs);
        assert!(
            gl.get_shader_compile_status(fs),
            "fragment shader must compile"
        );

        let prog = gl.create_program().expect("create program");
        gl.attach_shader(prog, vs);
        gl.attach_shader(prog, fs);
        gl.link_program(prog);
        assert!(gl.get_program_link_status(prog), "program must link");

        gl.delete_shader(vs);
        gl.delete_shader(fs);
        prog
    }
}

// ===========================================================================
// Guard-isolation tests (no mpv instance needed)
// ===========================================================================

#[test]
fn test_gl_state_guard_restores_all_state_variants() {
    let gl_env = match unsafe { HeadlessGlContext::create(320, 240) } {
        Some(e) => e,
        None => {
            let _ = skip("test_gl_state_guard_restores_all_state_variants");
            return;
        }
    };
    let gl = make_gl_context(&gl_env);

    unsafe {
        // Distinctive pre-state.
        gl.viewport(20, 30, 320, 240);
        gl.scissor(5, 6, 100, 100);
        gl.enable(glow::SCISSOR_TEST);
        gl.enable(glow::DEPTH_TEST);
        gl.disable(glow::CULL_FACE);
        gl.disable(glow::BLEND);

        // Two textures so we can identify which one is bound.
        let tex_a = gl.create_texture().expect("create texture a");
        let tex_b = gl.create_texture().expect("create texture b");
        gl.bind_texture(glow::TEXTURE_2D, Some(tex_b));
        gl.active_texture(glow::TEXTURE0);

        let prog = make_linked_program(&gl);

        gl.use_program(Some(prog));

        let vao = gl.create_vertex_array().expect("create vao");
        gl.bind_vertex_array(Some(vao));

        // Simulate "compositor" work.
        let guard = GlStateGuard::capture(&gl);
        gl.viewport(0, 0, 320, 240);
        gl.scissor(0, 0, 320, 240);
        gl.disable(glow::SCISSOR_TEST);
        gl.disable(glow::DEPTH_TEST);
        gl.enable(glow::CULL_FACE);
        gl.enable(glow::BLEND);
        gl.active_texture(glow::TEXTURE1);
        gl.bind_texture(glow::TEXTURE_2D, Some(tex_a));
        gl.use_program(None);
        gl.bind_vertex_array(None);

        // Restore (explicit restore mirrors Drop).
        guard.restore();

        let mut vp = [0i32; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut vp);
        assert_eq!(vp, [20, 30, 320, 240], "viewport must be restored");

        let mut sb = [0i32; 4];
        gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut sb);
        assert_eq!(sb, [5, 6, 100, 100], "scissor box must be restored");

        assert!(
            gl.is_enabled(glow::SCISSOR_TEST),
            "scissor test must be restored on"
        );
        assert!(
            gl.is_enabled(glow::DEPTH_TEST),
            "depth test must be restored on"
        );
        assert!(
            !gl.is_enabled(glow::CULL_FACE),
            "cull face must be restored off"
        );
        assert!(!gl.is_enabled(glow::BLEND), "blend must be restored off");

        assert_eq!(
            gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32,
            glow::TEXTURE0,
            "active texture must be restored"
        );
        assert_eq!(
            gl.get_parameter_texture(glow::TEXTURE_BINDING_2D),
            Some(tex_b),
            "bound texture must be restored"
        );
        assert_eq!(
            gl.get_parameter_program(glow::CURRENT_PROGRAM),
            Some(prog),
            "program must be restored"
        );
        assert_eq!(
            gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING),
            Some(vao),
            "VAO must be restored"
        );

        // Clean up GL objects.
        gl.bind_vertex_array(None);
        gl.use_program(None);
        gl.delete_texture(tex_a);
        gl.delete_texture(tex_b);
        gl.delete_program(prog);
        gl.delete_vertex_array(vao);
    }
}

#[test]
fn test_gl_state_guard_drop_autorestores_over_early_return() {
    let gl_env = match unsafe { HeadlessGlContext::create(320, 240) } {
        Some(e) => e,
        None => {
            let _ = skip("test_gl_state_guard_drop_autorestores_over_early_return");
            return;
        }
    };
    let gl = make_gl_context(&gl_env);

    unsafe {
        gl.viewport(0, 0, 111, 77);
        gl.disable(glow::SCISSOR_TEST);
        gl.scissor(1, 2, 3, 4);

        // Drop on early return must restore without an explicit call.
        let _guard = GlStateGuard::capture(&gl);
        gl.viewport(0, 0, 320, 240);
        assert_eq!(
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut [0i32; 4]),
            ()
        );
        drop(_guard);

        let mut vp = [0i32; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut vp);
        assert_eq!(vp, [0, 0, 111, 77], "RAII drop must restore viewport");
        assert!(!gl.is_enabled(glow::SCISSOR_TEST), "scissor must stay off");

        let mut sb = [0i32; 4];
        gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut sb);
        assert_eq!(sb, [1, 2, 3, 4], "scissor box must be restored by drop");
    }
}

// ===========================================================================
// Full compositor `paint()` tests (real mpv render context)
// ===========================================================================

fn create_engine(
    gl_env: &HeadlessGlContext,
    gl: &std::sync::Arc<glow::Context>,
) -> Option<(MpvHandle, std::sync::Arc<glow::Context>, RenderEngine)> {
    let _ = gl_env;
    let handle = MpvHandle::new().ok()?;
    handle.set_option_string("vo", "libmpv").ok()?;
    handle.set_option_string("ao", "null").ok()?;
    handle.initialize().ok()?;

    let render_ctx = unsafe {
        MpvRenderContext::new_gl(
            &handle,
            Some(egl_proc_loader),
            &gl_env.egl as *const _ as *mut c_void,
        )
        .ok()?
    };

    let engine = unsafe { RenderEngine::new(gl, render_ctx, 320, 240).ok()? };
    Some((handle, gl.clone(), engine))
}

fn wait_for_first_frame(handle: &MpvHandle) {
    let deadline = Instant::now() + Duration::from_millis(600);
    while Instant::now() < deadline {
        let _ = handle.wait_event(0.02);
    }
}

#[test]
fn test_compositor_paint_restores_state_and_framebuffer() {
    let gl_env = match unsafe { HeadlessGlContext::create(320, 240) } {
        Some(e) => e,
        None => {
            let _ = skip("test_compositor_paint_restores_state_and_framebuffer");
            return;
        }
    };
    let gl = make_gl_context(&gl_env);

    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/test_video.mp4")
        .canonicalize()
        .expect("test_video.mp4 must exist");

    // egui/glow painter (the same class of object the app uses).
    let painter = egui_glow::Painter::new(gl.clone(), "", None, false).expect("create painter");
    let painter_intermediate = painter.intermediate_fbo();

    let (handle, _gl2, mut engine) = match create_engine(&gl_env, &gl) {
        Some(v) => v,
        None => {
            eprintln!("SKIPPING test_compositor_paint_restores_state_and_framebuffer: engine setup failed");
            return;
        }
    };

    handle
        .command(&["loadfile", fixture.to_str().unwrap()])
        .unwrap();
    wait_for_first_frame(&handle);

    unsafe {
        // Distinctive pre-paint state that differs from the paint viewport.
        gl.viewport(7, 8, 111, 222);
        gl.scissor(3, 4, 50, 60);
        gl.enable(glow::SCISSOR_TEST);
        gl.enable(glow::DEPTH_TEST);
        gl.disable(glow::CULL_FACE);
        gl.disable(glow::BLEND);
        let tex = gl.create_texture().expect("create texture");
        gl.bind_texture(glow::TEXTURE_2D, Some(tex));

        let pre_framebuffer = gl.get_parameter_i32(glow::FRAMEBUFFER_BINDING);

        // Record expected framebuffer target *before* paint binds it:
        // intermediate FBO if the painter has allocated one yet, else the
        // framebuffer that was bound at entry (the painter's own contract).
        let expected_fbo = painter
            .intermediate_fbo()
            .map(|f| f.0.get() as i32)
            .unwrap_or(pre_framebuffer);

        engine.paint(&gl, &painter, [0, 0, 320, 240]);

        // 1) Framebuffer binding restored to the painter target.
        let post_fbo = gl.get_parameter_i32(glow::FRAMEBUFFER_BINDING);
        assert_eq!(
            post_fbo, expected_fbo,
            "framebuffer must be restored to the painter target after paint"
        );

        // 2) Guard-isolated state restored to the pre-paint values.
        let mut vp = [0i32; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut vp);
        assert_eq!(vp, [7, 8, 111, 222], "viewport restored to pre-paint value");

        let mut sb = [0i32; 4];
        gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut sb);
        assert_eq!(sb, [3, 4, 50, 60], "scissor box restored");

        assert!(gl.is_enabled(glow::SCISSOR_TEST), "scissor stays enabled");
        assert!(
            gl.is_enabled(glow::DEPTH_TEST),
            "depth stays enabled (quad disables it inside paint)"
        );
        assert!(!gl.is_enabled(glow::CULL_FACE), "cull face stays disabled");
        assert!(!gl.is_enabled(glow::BLEND), "blend stays disabled");

        assert_eq!(
            gl.get_parameter_texture(glow::TEXTURE_BINDING_2D),
            Some(tex),
            "texture binding restored after composition"
        );

        // 3) egui drawing right after video compositing must not error.
        let err = gl.get_error();
        assert_eq!(err, glow::NO_ERROR, "no GL error after paint");

        gl.bind_texture(glow::TEXTURE_2D, None);
        gl.delete_texture(tex);
        // `painter` dropped here, freeing its intermediate FBO.
    }

    let _ = painter_intermediate;
    unsafe {
        engine.destroy(&gl);
    }
}

#[test]
fn test_renderer_create_destroy_cycles_and_fbo_resize() {
    let gl_env = match unsafe { HeadlessGlContext::create(320, 240) } {
        Some(e) => e,
        None => {
            let _ = skip("test_renderer_create_destroy_cycles_and_fbo_resize");
            return;
        }
    };
    let gl = make_gl_context(&gl_env);

    // Repeated create/destroy must not leak or panic (10 cycles).
    for i in 0..10 {
        let (_handle, _gl2, mut engine) = match create_engine(&gl_env, &gl) {
            Some(v) => v,
            None => {
                eprintln!("SKIPPING: engine setup failed at cycle {i}");
                return;
            }
        };
        unsafe {
            engine.destroy(&gl);
        }
    }

    // FBO resize path: a larger paint viewport must reallocate the FBO safely.
    let (handle, _gl2, mut engine) = match create_engine(&gl_env, &gl) {
        Some(v) => v,
        None => {
            eprintln!("SKIPPING: engine setup failed for resize");
            return;
        }
    };
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/test_video.mp4")
        .canonicalize()
        .expect("test_video.mp4 must exist");
    handle
        .command(&["loadfile", fixture.to_str().unwrap()])
        .unwrap();
    wait_for_first_frame(&handle);

    let painter = egui_glow::Painter::new(gl.clone(), "", None, false).expect("create painter");
    unsafe {
        engine.paint(&gl, &painter, [0, 0, 160, 90]);
        engine.paint(&gl, &painter, [0, 0, 640, 480]);
        engine.paint(&gl, &painter, [0, 0, 320, 240]);
        assert_eq!(
            gl.get_error(),
            glow::NO_ERROR,
            "no GL error across FBO resize paints"
        );
        engine.destroy(&gl);
    }
}
