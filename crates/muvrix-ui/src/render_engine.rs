//! Rendering engine coordinating libmpv OpenGL playback and quad presentation.

use glow::HasContext as _;
use muvrix_player::MpvRenderContext;
use std::num::NonZeroU32;

use crate::fbo::OffscreenFbo;
use crate::quad::QuadRenderer;

/// Coordinates libmpv off-screen rendering and compositing into the egui glow pipeline.
pub struct RenderEngine {
    mpv_render_ctx: MpvRenderContext,
    fbo: OffscreenFbo,
    quad: QuadRenderer,
    frame_count: u64,
}

impl RenderEngine {
    /// Creates a new `RenderEngine` instance.
    ///
    /// # Safety
    ///
    /// Must be called with the OpenGL context current on the calling thread.
    pub unsafe fn new(
        gl: &glow::Context,
        mpv_render_ctx: MpvRenderContext,
        initial_width: u32,
        initial_height: u32,
    ) -> Result<Self, String> {
        let fbo = OffscreenFbo::new(gl, initial_width, initial_height)?;
        let quad = QuadRenderer::new(gl)?;

        Ok(Self {
            mpv_render_ctx,
            fbo,
            quad,
            frame_count: 0,
        })
    }

    /// Access the underlying mpv render context.
    pub fn mpv_render_ctx(&self) -> &MpvRenderContext {
        &self.mpv_render_ctx
    }

    /// Access the underlying mpv render context mutably.
    pub fn mpv_render_ctx_mut(&mut self) -> &mut MpvRenderContext {
        &mut self.mpv_render_ctx
    }

    /// Total video frames composited.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Paints the video frame enforcing the Framebuffer Restoration Contract.
    ///
    /// # Safety
    ///
    /// Must be called with the OpenGL context current on the calling thread.
    pub unsafe fn paint(
        &mut self,
        gl: &glow::Context,
        painter: &egui_glow::Painter,
        viewport_px: [i32; 4],
    ) {
        let [left, bottom, width, height] = viewport_px;
        if width <= 0 || height <= 0 {
            return;
        }

        // 1. Framebuffer Restoration Contract: Query active FBO
        let prev_fbo = gl.get_parameter_i32(glow::FRAMEBUFFER_BINDING);

        // 2. Ensure FBO dimensions match physical viewport
        self.fbo.ensure_size(gl, width as u32, height as u32);

        // 3. Render libmpv video frame into the off-screen FBO
        let fbo_params = libmpv_sys::mpv_opengl_fbo {
            fbo: self.fbo.fbo().0.get() as std::os::raw::c_int,
            w: width,
            h: height,
            internal_format: glow::RGBA8 as std::os::raw::c_int,
        };

        // Render into offscreen FBO.
        // flip_y: false (quad coordinates align bottom-left to bottom-left)
        // block_for_target_time: false (ensures UI thread does not freeze waiting on presentation timing)
        let _ = self.mpv_render_ctx.render_gl(fbo_params, false, false);
        self.mpv_render_ctx.report_swap();
        self.frame_count += 1;
        if self.frame_count % 30 == 1 {
            let center_pixel = self.fbo.read_pixel_sample(gl);
            log::info!(
                "Rendered frame #{}, viewport: [{}, {}, {}x{}], center RGBA: {:?}",
                self.frame_count,
                left,
                bottom,
                width,
                height,
                center_pixel
            );

            // Optional automated frame capture for verification testing
            if let Ok(capture_path) = std::env::var("MUVRIX_CAPTURE_FRAME_PNG") {
                if self.frame_count == 181
                    || (self.frame_count == 61 && !capture_path.contains("movie"))
                {
                    let count = self.frame_count;
                    if let Err(e) = self.fbo.save_screenshot_png(gl, &capture_path) {
                        log::error!("Failed to save screenshot: {e}");
                    } else {
                        log::info!("Successfully captured frame #{count} to {capture_path}");
                    }
                }
            }
        }

        // 4. Framebuffer Restoration Contract: Restore target framebuffer
        // Either the painter's intermediate FBO or the previously bound FBO
        let restore_target = painter
            .intermediate_fbo()
            .or_else(|| NonZeroU32::new(prev_fbo as u32).map(glow::NativeFramebuffer));
        gl.bind_framebuffer(glow::FRAMEBUFFER, restore_target);

        // 5. Composite video quad into the target viewport with RAII state isolation
        {
            let _state_guard = GlStateGuard::capture(gl);

            gl.viewport(left, bottom, width, height);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::BLEND);

            self.quad.render(gl, self.fbo.texture());
            // _state_guard restores viewport, scissor, blend, depth, cull, active texture,
            // bound texture, program, and VAO on drop.
        }

        // 6. Reset active texture unit to TEXTURE0
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, None);
    }

    /// Destroys OpenGL resources.
    ///
    /// # Safety
    ///
    /// Must be called with the OpenGL context current on the calling thread.
    pub unsafe fn destroy(&mut self, gl: &glow::Context) {
        self.mpv_render_ctx.destroy();
        self.fbo.destroy(gl);
        self.quad.destroy(gl);
    }
}

/// RAII guard that captures and isolates OpenGL pipeline state during video quad compositing.
/// Restores viewport, scissor box & test, depth test, cull face, blend state, active texture unit,
/// bound texture, shader program, and vertex array object upon drop or explicit restoration.
struct GlStateGuard<'a> {
    gl: &'a glow::Context,
    prev_viewport: [i32; 4],
    prev_scissor_box: [i32; 4],
    prev_scissor_test: bool,
    prev_depth_test: bool,
    prev_cull_face: bool,
    prev_blend: bool,
    prev_active_texture: u32,
    prev_texture_2d: Option<glow::NativeTexture>,
    prev_program: Option<glow::NativeProgram>,
    prev_vao: Option<glow::NativeVertexArray>,
}

impl<'a> GlStateGuard<'a> {
    unsafe fn capture(gl: &'a glow::Context) -> Self {
        let mut prev_viewport = [0i32; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut prev_viewport);

        let mut prev_scissor_box = [0i32; 4];
        gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut prev_scissor_box);

        let prev_scissor_test = gl.is_enabled(glow::SCISSOR_TEST);
        let prev_depth_test = gl.is_enabled(glow::DEPTH_TEST);
        let prev_cull_face = gl.is_enabled(glow::CULL_FACE);
        let prev_blend = gl.is_enabled(glow::BLEND);
        let prev_active_texture = gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32;
        let prev_texture_2d = gl.get_parameter_texture(glow::TEXTURE_BINDING_2D);
        let prev_program = gl.get_parameter_program(glow::CURRENT_PROGRAM);
        let prev_vao = gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING);

        Self {
            gl,
            prev_viewport,
            prev_scissor_box,
            prev_scissor_test,
            prev_depth_test,
            prev_cull_face,
            prev_blend,
            prev_active_texture,
            prev_texture_2d,
            prev_program,
            prev_vao,
        }
    }

    unsafe fn restore(&self) {
        let gl = self.gl;
        gl.viewport(
            self.prev_viewport[0],
            self.prev_viewport[1],
            self.prev_viewport[2],
            self.prev_viewport[3],
        );
        gl.scissor(
            self.prev_scissor_box[0],
            self.prev_scissor_box[1],
            self.prev_scissor_box[2],
            self.prev_scissor_box[3],
        );

        if self.prev_scissor_test {
            gl.enable(glow::SCISSOR_TEST);
        } else {
            gl.disable(glow::SCISSOR_TEST);
        }

        if self.prev_depth_test {
            gl.enable(glow::DEPTH_TEST);
        } else {
            gl.disable(glow::DEPTH_TEST);
        }

        if self.prev_cull_face {
            gl.enable(glow::CULL_FACE);
        } else {
            gl.disable(glow::CULL_FACE);
        }

        if self.prev_blend {
            gl.enable(glow::BLEND);
        } else {
            gl.disable(glow::BLEND);
        }

        gl.active_texture(self.prev_active_texture);
        gl.bind_texture(glow::TEXTURE_2D, self.prev_texture_2d);
        gl.use_program(self.prev_program);
        gl.bind_vertex_array(self.prev_vao);
    }
}

impl Drop for GlStateGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            self.restore();
        }
    }
}
