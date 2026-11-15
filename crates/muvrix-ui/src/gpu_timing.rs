//! Diagnostic-only, nonblocking GPU timer queries.
//!
//! Queries are begun/ended on the render thread and polled in later frames.
//! `GL_QUERY_RESULT_AVAILABLE` is checked; the code never waits for a result.
use std::collections::VecDeque;

use glow::HasContext as _;

const MAX_PENDING: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuStage {
    MpvRender = 1,
    Composite = 2,
}

struct Pending {
    query: glow::Query,
    stage: GpuStage,
    draw: u64,
    cpu_us: i64,
}

#[derive(Default)]
pub struct GpuTiming {
    pending: VecDeque<Pending>,
    enabled: bool,
    pub available_samples: u64,
    pub unavailable_polls: u64,
}

impl GpuTiming {
    pub fn new() -> Self {
        Self {
            enabled: std::env::var_os("MUVRIX_GPU_TIMING").is_some(),
            ..Self::default()
        }
    }

    /// Starts a timer query. Returns None if query creation fails.
    ///
    /// # Safety
    ///
    /// The OpenGL context must be current on the calling thread.
    pub unsafe fn begin(&self, gl: &glow::Context) -> Option<glow::Query> {
        if !self.enabled {
            return None;
        }
        let query = gl.create_query().ok()?;
        gl.begin_query(glow::TIME_ELAPSED, query);
        Some(query)
    }

    /// Ends a timer query and queues it for later polling.
    ///
    /// # Safety
    ///
    /// The OpenGL context must be current on the calling thread.
    pub unsafe fn end(
        &mut self,
        gl: &glow::Context,
        query: Option<glow::Query>,
        stage: GpuStage,
        draw: u64,
        cpu_us: i64,
    ) {
        let Some(query) = query else { return };
        gl.end_query(glow::TIME_ELAPSED);
        self.pending.push_back(Pending {
            query,
            stage,
            draw,
            cpu_us,
        });
        while self.pending.len() > MAX_PENDING {
            if let Some(old) = self.pending.pop_front() {
                gl.delete_query(old.query);
            }
        }
    }

    /// Polls completed queries without blocking. Stops at the first
    /// unavailable result so ordering is preserved.
    ///
    /// # Safety
    ///
    /// The OpenGL context must be current on the calling thread.
    pub unsafe fn poll(&mut self, gl: &glow::Context) {
        while let Some(front) = self.pending.front() {
            let available = gl.get_query_parameter_u32(front.query, glow::QUERY_RESULT_AVAILABLE);
            if available == 0 {
                self.unavailable_polls = self.unavailable_polls.saturating_add(1);
                break;
            }
            // TIMEstamp is 64-bit; glow's 64-bit getter is an unsafe raw-offset
            // API with no safe slice handle. The query durations here are far
            // below 2^32 ns (~4.29 s), so the low word is sufficient.
            let gpu_ns = gl.get_query_parameter_u32(front.query, glow::QUERY_RESULT) as u64;
            crate::causal_trace::record5(
                crate::causal_trace::Kind::GpuTime,
                front.draw,
                front.stage as i64,
                gpu_ns as i64,
                front.cpu_us,
                1,
                0,
                "available",
            );
            self.available_samples = self.available_samples.saturating_add(1);
            let old = self.pending.pop_front().unwrap();
            gl.delete_query(old.query);
        }
    }

    pub fn destroy(&mut self, gl: &glow::Context) {
        unsafe {
            while let Some(old) = self.pending.pop_front() {
                gl.delete_query(old.query);
            }
        }
    }
}
