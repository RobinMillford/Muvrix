//! Video + UI composition in the correct colour domain.
//!
//! The egui layer is always rendered into an `RGBA8` target with the video area
//! left transparent. This module then performs a single pass that
//!
//! 1. samples the video texture inside its target rectangle (letterboxed outside
//!    it) with the vertical flip the engine's FBO orientation requires,
//! 2. converts the premultiplied sRGB UI values to *linear light in nits*,
//!    encodes them with the PQ (ST 2084) curve when the surface is a PQ surface,
//! 3. and blends UI over video in that encoded domain.
//!
//! Blending sRGB UI values straight onto PQ video is the classic mistake: the
//! result is not just wrong-looking, it is wrong at any alpha < 1, which is
//! exactly where overlays, rounded corners and shadows live. Doing the blend
//! after the transfer function means translucency composites in the same space
//! the values are stored in.
//!
//! SDR output keeps the bit-exact path (`ui + video * (1 - alpha)`), because
//! there is no transfer-function mismatch to resolve there.

use glow::HasContext as _;

/// Output encoding of the window surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// sRGB-encoded window surface, SDR content.
    Sdr,
    /// PQ/BT.2020 window surface; the video texture already carries PQ values.
    Pq,
}

impl ColorMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sdr => "sdr",
            Self::Pq => "pq-bt2020",
        }
    }
}

const VERTEX_SRC: &str = r#"#version 300 es
precision highp float;
void main() {
    // Full-screen triangle; positions come from gl_VertexID so no buffers are
    // needed and no UI state can leak into the pass.
    vec2 p = vec2((gl_VertexID == 1) ? 3.0 : -1.0, (gl_VertexID == 2) ? 3.0 : -1.0);
    gl_Position = vec4(p, 0.0, 1.0);
}
"#;

const FRAGMENT_SRC: &str = r#"#version 300 es
precision highp float;

uniform vec2 u_screen;
uniform vec4 u_video_rect;   // x, y, w, h in window pixels (origin bottom-left)
uniform sampler2D u_video;
uniform sampler2D u_ui;
uniform int u_mode;          // 0 = SDR passthrough, 1 = PQ
uniform float u_white_nits;  // SDR reference white mapped onto the PQ surface

out vec4 fragColor;

vec3 srgb_to_linear(vec3 c) {
    vec3 lo = c / 12.92;
    vec3 hi = pow((c + 0.055) / 1.055, vec3(2.4));
    return mix(lo, hi, step(vec3(0.04045), c));
}

// ST 2084 (PQ) EOTF^-1. Input is linear light normalised to 10000 cd/m^2.
vec3 pq_encode(vec3 nits_normalised) {
    const float m1 = 0.1593017578125;
    const float m2 = 78.84375;
    const float c1 = 0.8359375;
    const float c2 = 18.8515625;
    const float c3 = 18.6875;
    vec3 L = clamp(nits_normalised, 0.0, 1.0);
    vec3 Lm = pow(L, vec3(m1));
    return pow((c1 + c2 * Lm) / (1.0 + c3 * Lm), vec3(m2));
}

void main() {
    vec2 frag = gl_FragCoord.xy;

    vec3 video = vec3(0.0);
    if (u_video_rect.z > 0.0 && u_video_rect.w > 0.0) {
        vec2 vuv = (frag - u_video_rect.xy) / u_video_rect.zw;
        if (vuv.x >= 0.0 && vuv.x <= 1.0 && vuv.y >= 0.0 && vuv.y <= 1.0) {
            // Render-engine FBOs are top-down; window coordinates are bottom-up.
            video = texture(u_video, vec2(vuv.x, 1.0 - vuv.y)).rgb;
        }
    }

    vec4 ui = texture(u_ui, frag / u_screen);

    if (u_mode == 1) {
        // Un-premultiply, linearise, scale to nits, then encode with PQ.
        vec3 ui_enc = ui.a > 0.0001 ? clamp(ui.rgb / ui.a, 0.0, 1.0) : vec3(0.0);
        vec3 ui_lin = srgb_to_linear(ui_enc) * (u_white_nits / 10000.0);
        vec3 ui_pq = pq_encode(ui_lin);
        fragColor = vec4(ui_pq * ui.a + video * (1.0 - ui.a), 1.0);
    } else {
        // Premultiplied "over" in the same 8-bit-ish encoded space both inputs use.
        fragColor = vec4(ui.rgb + video * (1.0 - ui.a), 1.0);
    }
}
"#;

/// Full-screen compositor pass.
pub struct Composite {
    program: glow::Program,
    vao: glow::VertexArray,
    u_screen: glow::UniformLocation,
    u_video_rect: glow::UniformLocation,
    u_video: glow::UniformLocation,
    u_ui: glow::UniformLocation,
    u_mode: glow::UniformLocation,
    u_white_nits: glow::UniformLocation,
}

impl Composite {
    /// Compiles the compositor.
    ///
    /// # Safety
    ///
    /// The host GL context must be current on the calling thread.
    pub unsafe fn new(gl: &glow::Context) -> Result<Self, String> {
        let vert = compile(gl, glow::VERTEX_SHADER, VERTEX_SRC)?;
        let frag = match compile(gl, glow::FRAGMENT_SHADER, FRAGMENT_SRC) {
            Ok(s) => s,
            Err(e) => {
                gl.delete_shader(vert);
                return Err(format!("composite fragment shader: {e}"));
            }
        };

        let program = gl
            .create_program()
            .map_err(|e| format!("create_program: {e}"))?;
        gl.attach_shader(program, vert);
        gl.attach_shader(program, frag);
        gl.link_program(program);
        gl.detach_shader(program, vert);
        gl.detach_shader(program, frag);
        gl.delete_shader(vert);
        gl.delete_shader(frag);

        if !gl.get_program_link_status(program) {
            let log = gl.get_program_info_log(program);
            gl.delete_program(program);
            return Err(format!("composite program link: {log}"));
        }

        let loc = |name: &str| -> Result<glow::UniformLocation, String> {
            gl.get_uniform_location(program, name)
                .ok_or_else(|| format!("missing uniform {name}"))
        };

        let vao = gl
            .create_vertex_array()
            .map_err(|e| format!("create_vertex_array: {e}"))?;

        log::debug!("composite shader linked");
        Ok(Self {
            program,
            vao,
            u_screen: loc("u_screen")?,
            u_video_rect: loc("u_video_rect")?,
            u_video: loc("u_video")?,
            u_ui: loc("u_ui")?,
            u_mode: loc("u_mode")?,
            u_white_nits: loc("u_white_nits")?,
        })
    }

    /// Draws video (inside `video_rect`, in pixels, origin bottom-left) under the
    /// premultiplied UI layer.
    ///
    /// # Safety
    ///
    /// Must be called with the host GL context current and the default
    /// framebuffer bound.
    #[allow(clippy::too_many_arguments)] // one explicit GL draw call keeps the inputs visible
    pub unsafe fn run(
        &self,
        gl: &glow::Context,
        video_texture: Option<glow::Texture>,
        ui_texture: glow::Texture,
        screen: (u32, u32),
        video_rect: [i32; 4],
        mode: ColorMode,
        white_nits: f32,
    ) {
        gl.disable(glow::BLEND);
        gl.disable(glow::SCISSOR_TEST);
        gl.disable(glow::DEPTH_TEST);
        gl.disable(glow::CULL_FACE);
        gl.viewport(0, 0, screen.0 as i32, screen.1 as i32);

        gl.use_program(Some(self.program));
        gl.bind_vertex_array(Some(self.vao));

        gl.uniform_2_f32(
            Some(&self.u_screen),
            screen.0.max(1) as f32,
            screen.1.max(1) as f32,
        );
        gl.uniform_4_f32(
            Some(&self.u_video_rect),
            video_rect[0] as f32,
            video_rect[1] as f32,
            video_rect[2] as f32,
            video_rect[3] as f32,
        );

        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, video_texture.or(Some(ui_texture)));
        gl.uniform_1_i32(Some(&self.u_video), 0);

        gl.active_texture(glow::TEXTURE1);
        gl.bind_texture(glow::TEXTURE_2D, Some(ui_texture));
        gl.uniform_1_i32(Some(&self.u_ui), 1);

        gl.uniform_1_i32(
            Some(&self.u_mode),
            if mode == ColorMode::Pq { 1 } else { 0 },
        );
        gl.uniform_1_f32(Some(&self.u_white_nits), white_nits);

        gl.draw_arrays(glow::TRIANGLES, 0, 3);

        gl.bind_vertex_array(None);
        gl.use_program(None);
        gl.active_texture(glow::TEXTURE1);
        gl.bind_texture(glow::TEXTURE_2D, None);
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, None);
    }

    /// Destroys GPU resources.
    ///
    /// # Safety
    ///
    /// Must be called with the host GL context current.
    pub unsafe fn destroy(&mut self, gl: &glow::Context) {
        gl.delete_program(self.program);
        gl.delete_vertex_array(self.vao);
    }
}

unsafe fn compile(gl: &glow::Context, kind: u32, source: &str) -> Result<glow::Shader, String> {
    let shader = gl
        .create_shader(kind)
        .map_err(|e| format!("create_shader: {e}"))?;
    gl.shader_source(shader, source);
    gl.compile_shader(shader);
    if !gl.get_shader_compile_status(shader) {
        let log = gl.get_shader_info_log(shader);
        gl.delete_shader(shader);
        return Err(log);
    }
    Ok(shader)
}
