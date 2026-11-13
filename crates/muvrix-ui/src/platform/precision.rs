//! Proof of the window surface's real quantisation.
//!
//! A 10-bit EGLConfig and an `RGBA16F` intermediate texture are both necessary
//! and neither is sufficient: what matters is how many distinct values survive a
//! round trip through the *window surface*. This probe writes two values that
//! differ by exactly one 10-bit step, copies the surface into a float target and
//! reads it back.
//!
//! * On a 10-bit surface the two values stay distinct (0.500488 vs 0.501465,
//!   i.e. 512/1023 vs 513/1023).
//! * On an 8-bit surface both collapse to 128/255 = 0.501961.
//!
//! Values, method and expected output match the Phase 1.9 surface POC
//! (`docs/phase-1.9/artifacts/poc/surface_poc.c`), which is what makes this a
//! regression test for the port rather than a new claim.

use glow::HasContext as _;

/// Result of the surface precision probe.
#[derive(Debug, Clone)]
pub struct PrecisionReport {
    /// Value written into the left half (512/1023).
    pub value_a: f32,
    /// Value written into the right half (513/1023).
    pub value_b: f32,
    /// Values read back from the surface.
    pub read_a: f32,
    pub read_b: f32,
    /// `true` when the two probes survived as distinct values.
    pub distinct: bool,
    /// Effective number of distinguishable steps implied by the readback. `None`
    /// when the probe could not be completed.
    pub implied_bits: Option<f32>,
    /// Non-fatal explanation when the probe could not run.
    pub note: Option<String>,
}

impl PrecisionReport {
    pub fn failed(reason: impl Into<String>) -> Self {
        Self {
            value_a: 512.0 / 1023.0,
            value_b: 513.0 / 1023.0,
            read_a: f32::NAN,
            read_b: f32::NAN,
            distinct: false,
            implied_bits: None,
            note: Some(reason.into()),
        }
    }

    pub fn summary(&self) -> String {
        if let Some(note) = &self.note {
            return format!("precision=UNPROBED ({note})");
        }
        format!(
            "precision distinct={} a={:.6} ({:.2}/1023) b={:.6} ({:.2}/1023){}",
            if self.distinct { "YES" } else { "NO" },
            self.read_a,
            self.read_a * 1023.0,
            self.read_b,
            self.read_b * 1023.0,
            self.implied_bits
                .map(|b| format!(" implied_bits={b:.1}"))
                .unwrap_or_default()
        )
    }
}

/// Runs the probe against the default framebuffer.
///
/// # Safety
///
/// The host GL context must be current and must have been created for this
/// surface, and the default framebuffer must not have been swapped yet (its
/// contents are only meaningful before the first `eglSwapBuffers`).
pub unsafe fn probe_surface_precision(
    gl: &glow::Context,
    width: u32,
    height: u32,
) -> PrecisionReport {
    let value_a = 512.0f32 / 1023.0;
    let value_b = 513.0f32 / 1023.0;
    let width = width.max(4);
    let height = height.max(4);

    // 1. Paint the two values into the window surface, left/right halves. A
    //    scissored clear avoids depending on shader compilation succeeding.
    gl.disable(glow::BLEND);
    gl.enable(glow::SCISSOR_TEST);
    gl.viewport(0, 0, width as i32, height as i32);

    gl.scissor(0, 0, (width / 2) as i32, height as i32);
    gl.clear_color(value_a, value_a, value_a, 1.0);
    gl.clear(glow::COLOR_BUFFER_BIT);

    gl.scissor(
        (width / 2) as i32,
        0,
        (width - width / 2) as i32,
        height as i32,
    );
    gl.clear_color(value_b, value_b, value_b, 1.0);
    gl.clear(glow::COLOR_BUFFER_BIT);

    gl.disable(glow::SCISSOR_TEST);

    let err = gl.get_error();
    if err != glow::NO_ERROR {
        return PrecisionReport::failed(format!("GL error 0x{err:x} while painting probe"));
    }

    // 2. Round-trip through a float target so the readback is not itself
    //    quantised. `GL_RGBA16F` is colour-renderable on ES3.
    let tex = match gl.create_texture() {
        Ok(t) => t,
        Err(e) => return PrecisionReport::failed(format!("create_texture: {e}")),
    };
    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::RGBA16F as i32,
        1,
        1,
        0,
        glow::RGBA,
        glow::FLOAT,
        glow::PixelUnpackData::Slice(None),
    );
    // Keep the texture bound on unit 0: `copy_tex_sub_image_2d` targets the
    // texture currently bound to the active unit, not the attachment.
    gl.active_texture(glow::TEXTURE0);

    let fbo = match gl.create_framebuffer() {
        Ok(f) => f,
        Err(e) => {
            gl.delete_texture(tex);
            return PrecisionReport::failed(format!("create_framebuffer: {e}"));
        }
    };
    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
    gl.framebuffer_texture_2d(
        glow::FRAMEBUFFER,
        glow::COLOR_ATTACHMENT0,
        glow::TEXTURE_2D,
        Some(tex),
        0,
    );
    let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
    if status != glow::FRAMEBUFFER_COMPLETE {
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        gl.delete_framebuffer(fbo);
        gl.delete_texture(tex);
        return PrecisionReport::failed(format!("float FBO incomplete: 0x{status:x}"));
    }
    gl.bind_framebuffer(glow::FRAMEBUFFER, None);

    // 3. Copy each probe pixel out of the window surface and read it as float.
    let read = |gl: &glow::Context, x: i32| -> Option<f32> {
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(fbo));
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None);
        gl.copy_tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, x, height as i32 / 2, 1, 1);
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        let mut bytes = [0u8; 4 * 4];
        gl.read_pixels(
            0,
            0,
            1,
            1,
            glow::RGBA,
            glow::FLOAT,
            glow::PixelPackData::Slice(Some(&mut bytes)),
        );
        let f: [f32; 4] = bytemuck::cast(bytes);
        if f[0].is_finite() {
            Some(f[0])
        } else {
            None
        }
    };

    let read_a = read(gl, width as i32 / 4);
    let read_b = read(gl, width as i32 * 3 / 4);

    gl.bind_framebuffer(glow::FRAMEBUFFER, None);
    gl.bind_texture(glow::TEXTURE_2D, None);
    gl.delete_framebuffer(fbo);
    gl.delete_texture(tex);

    match (read_a, read_b) {
        (Some(a), Some(b)) => {
            let distinct = (a - b).abs() > 1.0e-4;
            // One 10-bit step is 1/1023. If two adjacent steps separate, the
            // surface carries at least 10 bits of the value we wrote.
            let implied_bits = if distinct {
                Some(10.0)
            } else {
                // 8-bit quantisation merges 1/1023 neighbours.
                Some(8.0)
            };
            PrecisionReport {
                value_a,
                value_b,
                read_a: a,
                read_b: b,
                distinct,
                implied_bits,
                note: None,
            }
        }
        _ => PrecisionReport::failed("float readback of the surface failed"),
    }
}
