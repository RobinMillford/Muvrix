//! Off-screen framebuffer management for libmpv rendering.

use glow::HasContext as _;

/// Dedicated off-screen OpenGL Framebuffer Object for libmpv's render target.
///
/// The internal format matters for colour: `GL_RGBA8` quantises PQ values to 8
/// bits before the compositor ever sees them, which is enough to band dark
/// HDR10 content. `GL_RGBA16F` keeps the engine's float output intact and is
/// selected whenever the surface is a PQ surface.
pub struct OffscreenFbo {
    fbo: glow::Framebuffer,
    texture: glow::Texture,
    width: u32,
    height: u32,
    internal_format: i32,
    upload_type: u32,
}

impl OffscreenFbo {
    /// Creates a new offscreen framebuffer of the given dimensions (`GL_RGBA8`).
    ///
    /// # Safety
    ///
    /// The OpenGL context must be current on the calling thread.
    pub unsafe fn new(gl: &glow::Context, width: u32, height: u32) -> Result<Self, String> {
        Self::new_with_format(gl, width, height, glow::RGBA8 as i32)
    }

    /// Creates an offscreen framebuffer with an explicit internal format.
    ///
    /// # Safety
    ///
    /// The OpenGL context must be current on the calling thread.
    pub unsafe fn new_with_format(
        gl: &glow::Context,
        width: u32,
        height: u32,
        internal_format: i32,
    ) -> Result<Self, String> {
        let width = width.max(1);
        let height = height.max(1);
        let upload_type =
            if internal_format == glow::RGBA16F as i32 || internal_format == glow::RGBA32F as i32 {
                glow::FLOAT
            } else {
                glow::UNSIGNED_BYTE
            };

        // 1. Create color texture
        let texture = gl.create_texture().map_err(|e| e.to_string())?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            internal_format,
            width as i32,
            height as i32,
            0,
            glow::RGBA,
            upload_type,
            glow::PixelUnpackData::Slice(None),
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.bind_texture(glow::TEXTURE_2D, None);

        // 2. Create and attach framebuffer
        let fbo = gl.create_framebuffer().map_err(|e| e.to_string())?;
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );

        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);

        if status != glow::FRAMEBUFFER_COMPLETE {
            gl.delete_texture(texture);
            gl.delete_framebuffer(fbo);
            return Err(format!("Framebuffer incomplete with status: 0x{status:X}"));
        }

        log::info!(
            "Created offscreen FBO ({width}x{height}, internal_format=0x{internal_format:x})"
        );

        Ok(Self {
            fbo,
            texture,
            width,
            height,
            internal_format,
            upload_type,
        })
    }

    /// Internal format the engine must render into (`mpv_opengl_fbo.internal_format`).
    pub fn internal_format(&self) -> i32 {
        self.internal_format
    }

    /// Resizes the framebuffer texture storage if dimensions have changed.
    ///
    /// # Safety
    ///
    /// The OpenGL context must be current on the calling thread.
    pub unsafe fn ensure_size(&mut self, gl: &glow::Context, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);

        if self.width == width && self.height == height {
            return;
        }

        gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            self.internal_format,
            width as i32,
            height as i32,
            0,
            glow::RGBA,
            self.upload_type,
            glow::PixelUnpackData::Slice(None),
        );
        gl.bind_texture(glow::TEXTURE_2D, None);

        self.width = width;
        self.height = height;

        log::debug!("Resized offscreen FBO to {width}x{height}");
    }

    /// Returns the underlying OpenGL Framebuffer handle.
    pub fn fbo(&self) -> glow::Framebuffer {
        self.fbo
    }

    /// Returns the underlying color texture handle.
    pub fn texture(&self) -> glow::Texture {
        self.texture
    }

    /// Returns current width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns current height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Reads a 1x1 pixel sample from the center of the framebuffer for verification.
    ///
    /// # Safety
    ///
    /// The OpenGL context must be current on the calling thread.
    pub unsafe fn read_pixel_sample(&self, gl: &glow::Context) -> [u8; 4] {
        let mut pixel = [0u8; 4];
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
        gl.read_pixels(
            (self.width / 2) as i32,
            (self.height / 2) as i32,
            1,
            1,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut pixel)),
        );
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        pixel
    }

    /// Reads the entire framebuffer and saves it to a PNG file.
    ///
    /// # Safety
    ///
    /// The OpenGL context must be current on the calling thread.
    pub unsafe fn save_screenshot_png(&self, gl: &glow::Context, path: &str) -> Result<(), String> {
        let mut pixels = vec![0u8; (self.width * self.height * 4) as usize];
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
        gl.read_pixels(
            0,
            0,
            self.width as i32,
            self.height as i32,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut pixels)),
        );
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);

        let stride = (self.width * 4) as usize;
        let mut flipped = Vec::with_capacity(pixels.len());
        for row in pixels.chunks_exact(stride).rev() {
            flipped.extend_from_slice(row);
        }

        image::save_buffer(
            path,
            &flipped,
            self.width,
            self.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| e.to_string())
    }

    /// Frees OpenGL resources.
    ///
    /// # Safety
    ///
    /// Must be called with the OpenGL context current on the calling thread.
    pub unsafe fn destroy(&mut self, gl: &glow::Context) {
        gl.delete_texture(self.texture);
        gl.delete_framebuffer(self.fbo);
    }
}
