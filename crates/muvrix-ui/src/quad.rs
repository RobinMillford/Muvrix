//! Minimal OpenGL 3.3+ quad renderer for texture presentation.

use glow::HasContext as _;

const VERTEX_SHADER_SRC: &str = r#"#version 330 core
layout (location = 0) in vec2 a_pos;
layout (location = 1) in vec2 a_uv;
out vec2 v_uv;

void main() {
    v_uv = a_uv;
    gl_Position = vec4(a_pos, 0.0, 1.0);
}
"#;

const FRAGMENT_SHADER_SRC: &str = r#"#version 330 core
in vec2 v_uv;
out vec4 FragColor;
uniform sampler2D u_texture;

void main() {
    FragColor = texture(u_texture, v_uv);
}
"#;

#[rustfmt::skip]
const QUAD_VERTICES: [f32; 24] = [
    // pos.x, pos.y, uv.u, uv.v
    -1.0, -1.0,  0.0, 0.0,
     1.0, -1.0,  1.0, 0.0,
     1.0,  1.0,  1.0, 1.0,

    -1.0, -1.0,  0.0, 0.0,
     1.0,  1.0,  1.0, 1.0,
    -1.0,  1.0,  0.0, 1.0,
];

/// Shader program and vertex arrays for rendering a textured screen quad.
pub struct QuadRenderer {
    program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    u_texture: glow::UniformLocation,
}

impl QuadRenderer {
    /// Creates and compiles the quad shader pipeline.
    ///
    /// # Safety
    ///
    /// The OpenGL context must be current on the calling thread.
    pub unsafe fn new(gl: &glow::Context) -> Result<Self, String> {
        let vert = gl
            .create_shader(glow::VERTEX_SHADER)
            .map_err(|e| e.to_string())?;
        gl.shader_source(vert, VERTEX_SHADER_SRC);
        gl.compile_shader(vert);
        if !gl.get_shader_compile_status(vert) {
            let log = gl.get_shader_info_log(vert);
            gl.delete_shader(vert);
            return Err(format!("Vertex shader compile error: {log}"));
        }

        let frag = gl
            .create_shader(glow::FRAGMENT_SHADER)
            .map_err(|e| e.to_string())?;
        gl.shader_source(frag, FRAGMENT_SHADER_SRC);
        gl.compile_shader(frag);
        if !gl.get_shader_compile_status(frag) {
            let log = gl.get_shader_info_log(frag);
            gl.delete_shader(vert);
            gl.delete_shader(frag);
            return Err(format!("Fragment shader compile error: {log}"));
        }

        let program = gl.create_program().map_err(|e| e.to_string())?;
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
            return Err(format!("Program link error: {log}"));
        }

        let u_texture = gl
            .get_uniform_location(program, "u_texture")
            .ok_or_else(|| "Failed to find uniform 'u_texture'".to_string())?;

        let vao = gl.create_vertex_array().map_err(|e| e.to_string())?;
        let vbo = gl.create_buffer().map_err(|e| e.to_string())?;

        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(&QUAD_VERTICES),
            glow::STATIC_DRAW,
        );

        let stride = (4 * std::mem::size_of::<f32>()) as i32;
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);

        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(
            1,
            2,
            glow::FLOAT,
            false,
            stride,
            (2 * std::mem::size_of::<f32>()) as i32,
        );

        gl.bind_vertex_array(None);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);

        log::info!("Compiled and linked OpenGL 3.3+ quad shader program");

        Ok(Self {
            program,
            vao,
            vbo,
            u_texture,
        })
    }

    /// Draws the quad with the given texture.
    ///
    /// # Safety
    ///
    /// The OpenGL context must be current on the calling thread.
    pub unsafe fn render(&self, gl: &glow::Context, texture: glow::Texture) {
        gl.use_program(Some(self.program));

        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.uniform_1_i32(Some(&self.u_texture), 0);

        gl.bind_vertex_array(Some(self.vao));
        gl.draw_arrays(glow::TRIANGLES, 0, 6);

        gl.bind_vertex_array(None);
        gl.bind_texture(glow::TEXTURE_2D, None);
        gl.use_program(None);
    }

    /// Destroys OpenGL shader and buffer resources.
    ///
    /// # Safety
    ///
    /// Must be called with the OpenGL context current on the calling thread.
    pub unsafe fn destroy(&mut self, gl: &glow::Context) {
        gl.delete_program(self.program);
        gl.delete_vertex_array(self.vao);
        gl.delete_buffer(self.vbo);
    }
}
