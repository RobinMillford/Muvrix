//! WP4 — per-second telemetry aggregation and CSV emission.
//!
//! ## Why this module owns a thread
//!
//! Phase 2.1 recorded a failure mode where telemetry stopped progressing during
//! long interruptions. The cause was structural: the per-second row was emitted
//! from inside the paint callback, so anything that stopped painting also stopped
//! measurement — precisely when measurement matters most.
//!
//! The emitter now runs on its own thread ([`TelemetryWorker`]). It reads only
//! lock-free counters, a `try_lock`ed snapshot of the presentation state machine,
//! a bounded timing-sample buffer and the clocks. It never calls EGL, GL or
//! libmpv, and it never waits on the render thread: a contended snapshot lock
//! means the previous frame's snapshot is used for that sample.
//!
//! ## Counter semantics (schema 2.4)
//!
//! The Phase 2.3 schema used `presented_frames` for successful EGL swaps, which
//! made it look like video frames were presented 2.5x faster than they were
//! rendered. The counters are now named for exactly one event each. See
//! `docs/phase-2.2/artifacts/telemetry-csv-schema.md`.
//!
//! Every emitted row is validated against the header before it is written; a
//! mismatch is a telemetry-emission defect and is logged rather than written, so
//! a malformed file can never look like data.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::clock::ClockSample;
use crate::presentation::{PresentationSnapshot, PresentationStateMachine, SnapshotAges};

/// Canonical CSV header, exactly as specified (one physical line, 56 fields).
pub const CANONICAL_HEADER: &str = "timestamp,run,t,-t_since_start,state,state_age_s,\
last_present_age_s,last_swap_age_s,last_callback_age_s,unavailable_episodes,\
unavailable_recoveries,state_last_event_ts,update_callbacks_this_second,\
update_callbacks_total,new_video_frames_this_second,new_video_frames_total,\
fbo_render_calls_this_second,fbo_render_calls_total,egl_swap_attempts_this_second,\
egl_swap_attempts_total,egl_swap_successes_this_second,egl_swap_successes_total,\
egl_swap_failures_this_second,egl_swap_failures_total,\
mpv_report_swap_calls_this_second,mpv_report_swap_calls_total,\
wayland_presented_feedback_total,swap_call_duration_p50_ms,swap_call_duration_p95_ms,\
swap_call_duration_max_ms,inter_swap_gap_p50_ms,inter_swap_gap_p95_ms,\
inter_swap_gap_p99_ms,inter_swap_gap_max_ms,frame_interval_p50_ms,\
frame_interval_p95_ms,frame_interval_p99_ms,frame_interval_max_ms,\
intervals_exceeding_1_5x,intervals_exceeding_2x,intervals_exceeding_3x,\
target_frame_interval_ms,wake_events_this_second,wake_events_total,\
redraw_requests_this_second,redraw_requests_total,\
no_frame_fallback_deadlines_this_second,no_frame_fallback_deadlines_total,\
dropped_frames_this_second,dropped_frames_total,decoder_dropped_total,\
clock_monotonic_s,clock_boottime_s,clock_realtime_s,boottime_minus_monotonic_s,notes";

/// Authoritative field count of [`CANONICAL_HEADER`].
pub const CANONICAL_FIELD_COUNT: usize = 56;

/// Sentinel written when a measurement is genuinely unavailable.
///
/// It is not `0.0`: a missing boottime reading and a boottime of zero must never
/// look alike.
pub const UNAVAILABLE: f64 = -1.0;

/// Integer sentinel for the same purpose in integer columns.
pub const UNAVAILABLE_INT: i64 = -1;

/// Maximum timing samples retained per second. Bounds memory on pathological
/// frame rates; the percentiles are unaffected at any realistic rate.
const MAX_SAMPLES_PER_SECOND: usize = 4096;

/// Splits a CSV line into fields.
///
/// The writer sanitizes the free-text `notes` column (commas replaced), so a
/// plain split is exact for rows this module produces.
pub fn split_fields(line: &str) -> Vec<&str> {
    line.trim_end_matches(['\r', '\n']).split(',').collect()
}

/// Number of fields in a CSV line.
pub fn field_count(line: &str) -> usize {
    split_fields(line).len()
}

/// Header field count, computed from the constant rather than hard-coded twice.
pub fn header_field_count() -> usize {
    field_count(CANONICAL_HEADER)
}

/// Validation result for a prospective row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldCountDefect {
    pub expected: usize,
    pub actual: usize,
}

impl std::fmt::Display for FieldCountDefect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "telemetry row has {} fields, canonical header has {}",
            self.actual, self.expected
        )
    }
}

/// Validates a row against the canonical header field count.
pub fn validate_row(line: &str) -> Result<(), FieldCountDefect> {
    let actual = field_count(line);
    if actual == CANONICAL_FIELD_COUNT {
        Ok(())
    } else {
        Err(FieldCountDefect {
            expected: CANONICAL_FIELD_COUNT,
            actual,
        })
    }
}

/// Replaces characters that would break the CSV structure.
pub fn sanitize_note(note: &str) -> String {
    note.replace([',', '\n', '\r'], ";")
}

/// Nearest-rank percentiles of a timing sample set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Percentiles {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
    pub count: usize,
}

/// Computes p50/p95/p99/max, returning `None` when there are no samples.
pub fn percentiles(samples_us: &[u64]) -> Option<Percentiles> {
    if samples_us.is_empty() {
        return None;
    }
    let mut sorted = samples_us.to_vec();
    sorted.sort_unstable();

    let at = |q: f64| -> f64 {
        let idx = ((q * sorted.len() as f64).ceil() as usize)
            .saturating_sub(1)
            .min(sorted.len() - 1);
        sorted[idx] as f64 / 1000.0
    };
    Some(Percentiles {
        p50: at(0.50),
        p95: at(0.95),
        p99: at(0.99),
        max: *sorted.last().unwrap() as f64 / 1000.0,
        count: sorted.len(),
    })
}

/// Formatting helper: unavailable values print as `-1`, never as `0`.
fn fmt_opt(v: Option<f64>) -> String {
    match v {
        Some(v) => format!("{v:.3}"),
        None => format!("{UNAVAILABLE:.0}"),
    }
}

/// UTC timestamp in ISO-8601 (`YYYY-MM-DDTHH:MM:SSZ`).
///
/// Implemented locally because the crate has no chrono/time dependency and the
/// civil-date conversion is a dozen lines.
pub fn iso8601_utc(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let sod = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

/// Howard Hinnant's `civil_from_days` (days since 1970-01-01 → y/m/d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Cumulative counters sampled from the runtime once per second.
///
/// Each field is one event and one event only:
///
/// * `update_callbacks_total` — mpv's update callback fired.
/// * `new_video_frames_total` — libmpv rendered a *new* video frame into the
///   working target (`mpv_render_context_render` with a frame available).
/// * `fbo_render_calls_total` — a composition pass ran into the window target.
///   This advances on repaints of an unchanged video frame too.
/// * `egl_swap_attempts_total` / `_successes_total` / `_failures_total` —
///   `eglSwapBuffers` calls, their outcomes.
/// * `mpv_report_swap_calls_total` — `mpv_render_context_report_swap` calls,
///   which the API defines per *flipped frame*, not per buffer swap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub update_callbacks_total: u64,
    pub new_video_frames_total: u64,
    pub fbo_render_calls_total: u64,
    pub egl_swap_attempts_total: u64,
    pub egl_swap_successes_total: u64,
    pub egl_swap_failures_total: u64,
    pub mpv_report_swap_calls_total: u64,
    pub wake_events_total: u64,
    pub redraw_requests_total: u64,
    pub no_frame_fallback_deadlines_total: u64,
    pub dropped_frames_total: u64,
    /// Cumulative decoder drops; `-1` when the run cannot observe them.
    pub decoder_dropped_total: i64,
}

/// Per-second deltas of [`Counters`]. `decoder_dropped_total` is cumulative-only
/// and is not repeated as a delta.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CounterDeltas {
    pub update_callbacks: u64,
    pub new_video_frames: u64,
    pub fbo_render_calls: u64,
    pub egl_swap_attempts: u64,
    pub egl_swap_successes: u64,
    pub egl_swap_failures: u64,
    pub mpv_report_swap_calls: u64,
    pub wake_events: u64,
    pub redraw_requests: u64,
    pub no_frame_fallback_deadlines: u64,
    pub dropped_frames: u64,
}

impl CounterDeltas {
    fn between(prev: &Counters, now: &Counters) -> Self {
        Self {
            update_callbacks: now
                .update_callbacks_total
                .saturating_sub(prev.update_callbacks_total),
            new_video_frames: now
                .new_video_frames_total
                .saturating_sub(prev.new_video_frames_total),
            fbo_render_calls: now
                .fbo_render_calls_total
                .saturating_sub(prev.fbo_render_calls_total),
            egl_swap_attempts: now
                .egl_swap_attempts_total
                .saturating_sub(prev.egl_swap_attempts_total),
            egl_swap_successes: now
                .egl_swap_successes_total
                .saturating_sub(prev.egl_swap_successes_total),
            egl_swap_failures: now
                .egl_swap_failures_total
                .saturating_sub(prev.egl_swap_failures_total),
            mpv_report_swap_calls: now
                .mpv_report_swap_calls_total
                .saturating_sub(prev.mpv_report_swap_calls_total),
            wake_events: now.wake_events_total.saturating_sub(prev.wake_events_total),
            redraw_requests: now
                .redraw_requests_total
                .saturating_sub(prev.redraw_requests_total),
            no_frame_fallback_deadlines: now
                .no_frame_fallback_deadlines_total
                .saturating_sub(prev.no_frame_fallback_deadlines_total),
            dropped_frames: now
                .dropped_frames_total
                .saturating_sub(prev.dropped_frames_total),
        }
    }
}

/// One fully-formed telemetry row.
#[derive(Debug, Clone)]
pub struct TelemetryRow {
    pub timestamp: String,
    pub run: String,
    pub t_s: f64,
    pub state: &'static str,
    pub ages: SnapshotAges,
    pub snapshot: PresentationSnapshot,
    pub counters: Counters,
    pub delta: CounterDeltas,
    /// `None` until Wayland presentation feedback is actually collected.
    pub wayland_presented_feedback_total: Option<u64>,
    pub swap_call: Option<Percentiles>,
    pub inter_swap_gap: Option<Percentiles>,
    pub frame_interval: Option<Percentiles>,
    pub intervals_exceeding_1_5x: u64,
    pub intervals_exceeding_2x: u64,
    pub intervals_exceeding_3x: u64,
    pub target_frame_interval_ms: f64,
    pub clock: ClockSample,
    pub notes: String,
}

impl TelemetryRow {
    /// Builds one row from a sample of the runtime.
    ///
    /// `now` is the sampling instant. Event instants inside `snapshot` are the
    /// real ones, so `last_*_age_s` are ages *at sampling time* — not a value
    /// that is structurally zero because the sample and the event share a
    /// timestamp.
    #[allow(clippy::too_many_arguments)] // deliberate row constructor; splitting it would obscure the schema
    pub fn sample(
        now: Instant,
        started: Instant,
        run: &str,
        target_frame_interval: Duration,
        counters: Counters,
        prev: &Counters,
        snapshot: PresentationSnapshot,
        timing: &TimingSamples,
        clock: ClockSample,
        wayland_presented_feedback_total: Option<u64>,
    ) -> Self {
        let ages = PresentationStateMachine::snapshot_ages(&snapshot, now);
        let target_ms = target_frame_interval.as_secs_f64() * 1000.0;
        let (x15, x2, x3) = exceedances(&timing.frame_interval_us, target_ms);
        Self {
            timestamp: iso8601_utc(SystemTime::now()),
            run: run.to_string(),
            t_s: now.saturating_duration_since(started).as_secs_f64(),
            state: snapshot.state.as_str(),
            ages,
            snapshot,
            counters,
            delta: CounterDeltas::between(prev, &counters),
            wayland_presented_feedback_total,
            swap_call: percentiles(&timing.swap_call_us),
            inter_swap_gap: percentiles(&timing.inter_swap_gap_us),
            frame_interval: percentiles(&timing.frame_interval_us),
            intervals_exceeding_1_5x: x15,
            intervals_exceeding_2x: x2,
            intervals_exceeding_3x: x3,
            target_frame_interval_ms: target_ms,
            clock,
            notes: String::new(),
        }
    }

    /// Serializes to exactly [`CANONICAL_FIELD_COUNT`] comma-separated fields.
    pub fn to_csv_line(&self) -> String {
        let c = &self.counters;
        let d = &self.delta;
        let pct = |p: Option<Percentiles>| match p {
            Some(p) => (Some(p.p50), Some(p.p95), Some(p.p99), Some(p.max)),
            None => (None, None, None, None),
        };
        let swap = pct(self.swap_call);
        let gap = pct(self.inter_swap_gap);
        let interval = pct(self.frame_interval);

        let fields: [String; CANONICAL_FIELD_COUNT] = [
            self.timestamp.clone(),
            self.run.clone(),
            format!("{:.3}", self.t_s),
            format!("{:.3}", self.t_s),
            self.state.to_string(),
            format!("{:.3}", self.ages.state_age_s),
            fmt_opt(self.ages.last_present_age_s),
            fmt_opt(self.ages.last_swap_age_s),
            fmt_opt(self.ages.last_callback_age_s),
            self.snapshot.unavailable_episodes.to_string(),
            self.snapshot.unavailable_recoveries.to_string(),
            format!("{:.3}", self.snapshot.last_event_t_s),
            d.update_callbacks.to_string(),
            c.update_callbacks_total.to_string(),
            d.new_video_frames.to_string(),
            c.new_video_frames_total.to_string(),
            d.fbo_render_calls.to_string(),
            c.fbo_render_calls_total.to_string(),
            d.egl_swap_attempts.to_string(),
            c.egl_swap_attempts_total.to_string(),
            d.egl_swap_successes.to_string(),
            c.egl_swap_successes_total.to_string(),
            d.egl_swap_failures.to_string(),
            c.egl_swap_failures_total.to_string(),
            d.mpv_report_swap_calls.to_string(),
            c.mpv_report_swap_calls_total.to_string(),
            match self.wayland_presented_feedback_total {
                Some(v) => v.to_string(),
                None => UNAVAILABLE_INT.to_string(),
            },
            fmt_opt(swap.0),
            fmt_opt(swap.1),
            fmt_opt(swap.3),
            fmt_opt(gap.0),
            fmt_opt(gap.1),
            fmt_opt(gap.2),
            fmt_opt(gap.3),
            fmt_opt(interval.0),
            fmt_opt(interval.1),
            fmt_opt(interval.2),
            fmt_opt(interval.3),
            self.intervals_exceeding_1_5x.to_string(),
            self.intervals_exceeding_2x.to_string(),
            self.intervals_exceeding_3x.to_string(),
            format!("{:.3}", self.target_frame_interval_ms),
            d.wake_events.to_string(),
            c.wake_events_total.to_string(),
            d.redraw_requests.to_string(),
            c.redraw_requests_total.to_string(),
            d.no_frame_fallback_deadlines.to_string(),
            c.no_frame_fallback_deadlines_total.to_string(),
            d.dropped_frames.to_string(),
            c.dropped_frames_total.to_string(),
            c.decoder_dropped_total.to_string(),
            fmt_opt(self.clock.monotonic_s),
            fmt_opt(self.clock.boottime_s),
            fmt_opt(self.clock.realtime_s),
            fmt_opt(self.clock.boottime_minus_monotonic_s),
            sanitize_note(&self.notes),
        ];
        fields.join(",")
    }
}

/// Appends validated rows to one CSV file.
pub struct TelemetryWriter {
    path: PathBuf,
    lines: u64,
}

impl TelemetryWriter {
    /// Creates/truncates the file and writes the canonical header.
    pub fn create(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut file = std::fs::File::create(&path)?;
        file.write_all(CANONICAL_HEADER.as_bytes())?;
        file.write_all(b"\n")?;
        Ok(Self { path, lines: 0 })
    }

    pub fn lines(&self) -> u64 {
        self.lines
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Validates and appends one row.
    ///
    /// A row that does not match the canonical field count is refused: writing
    /// it would make the file silently unparsable.
    pub fn write_line(&mut self, line: &str) -> Result<(), FieldCountDefect> {
        validate_row(line)?;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&self.path)
            .map_err(|_| FieldCountDefect {
                expected: CANONICAL_FIELD_COUNT,
                actual: field_count(line),
            })?;
        if file.write_all(line.as_bytes()).is_err() || file.write_all(b"\n").is_err() {
            return Err(FieldCountDefect {
                expected: CANONICAL_FIELD_COUNT,
                actual: field_count(line),
            });
        }
        self.lines += 1;
        Ok(())
    }

    /// Writes a row after validation, logging (not hiding) a defect.
    pub fn write_row(&mut self, row: &TelemetryRow) -> bool {
        let line = row.to_csv_line();
        match self.write_line(&line) {
            Ok(()) => true,
            Err(defect) => {
                log::error!("telemetry emission defect: {defect}; row not written");
                false
            }
        }
    }
}

/// Timing samples accumulated between samples of the runtime.
///
/// Written on the render thread (bounded push), drained by the telemetry thread.
#[derive(Debug, Default)]
pub struct TimingSamples {
    pub swap_call_us: Vec<u64>,
    /// Present-to-present gaps (the `inter_swap_gap_*` columns).
    pub inter_swap_gap_us: Vec<u64>,
    /// The same present-to-present gaps, reported as `frame_interval_*` and
    /// compared against the display period. Present cadence, not source-frame
    /// cadence; see [`SharedMetrics::note_present_gap`].
    pub frame_interval_us: Vec<u64>,
    pub longest_gap_ms: f64,
    /// Samples discarded because the buffer was full or the lock was contended.
    pub dropped_samples: u64,
}

impl TimingSamples {
    fn push(buf: &mut Vec<u64>, value: u64) -> bool {
        if buf.len() < MAX_SAMPLES_PER_SECOND {
            buf.push(value);
            true
        } else {
            false
        }
    }

    /// Drains the buffers into owned vectors, leaving the accumulator empty.
    pub fn take(&mut self) -> TimingSamples {
        TimingSamples {
            swap_call_us: std::mem::take(&mut self.swap_call_us),
            inter_swap_gap_us: std::mem::take(&mut self.inter_swap_gap_us),
            frame_interval_us: std::mem::take(&mut self.frame_interval_us),
            longest_gap_ms: std::mem::replace(&mut self.longest_gap_ms, 0.0),
            dropped_samples: std::mem::take(&mut self.dropped_samples),
        }
    }
}

/// State shared between the render thread (writer) and the telemetry thread
/// (reader).
///
/// The render thread never blocks: counters are atomics, the snapshot uses
/// `try_lock` and the timing buffer uses `try_lock`. Losing a publish because the
/// telemetry thread is mid-sample costs at most one frame of snapshot accuracy,
/// never a frame of rendering.
#[derive(Debug)]
pub struct SharedMetrics {
    update_callbacks: AtomicU64,
    new_video_frames: AtomicU64,
    fbo_render_calls: AtomicU64,
    egl_swap_attempts: AtomicU64,
    egl_swap_successes: AtomicU64,
    egl_swap_failures: AtomicU64,
    mpv_report_swap_calls: AtomicU64,
    wake_events: AtomicU64,
    redraw_requests: AtomicU64,
    no_frame_fallback_deadlines: AtomicU64,
    dropped_frames: AtomicU64,
    decoder_dropped: AtomicI64,
    presentation_feedback_available: AtomicBool,
    presentation_feedback_total: AtomicU64,
    snapshot: Mutex<PresentationSnapshot>,
    timing: Mutex<TimingSamples>,
    rows_written: AtomicU64,
    /// Incremented once per paint. The worker uses it as a liveness signal: mpv
    /// waits for a swap report, and if the event loop happens to park with no
    /// deadline armed, nothing asks for that paint and the process deadlocks
    /// (reproduced on the Phase 2.3 build as well as this one).
    paint_seq: AtomicU64,
    /// Set by the worker when the configured run deadline passes. Read by the
    /// event loop, so termination does not require a paint (Phase 2.5, Part 7).
    deadline_expired: AtomicBool,
    stopping: AtomicBool,
    /// Set when a publish could not acquire a lock; proves the render thread was
    /// never the one waiting.
    publish_skips: AtomicU64,
}

fn store(a: &AtomicU64, v: u64) {
    a.store(v, Ordering::Relaxed);
}

impl SharedMetrics {
    pub fn new(initial: PresentationSnapshot) -> Self {
        Self {
            update_callbacks: AtomicU64::new(0),
            new_video_frames: AtomicU64::new(0),
            fbo_render_calls: AtomicU64::new(0),
            egl_swap_attempts: AtomicU64::new(0),
            egl_swap_successes: AtomicU64::new(0),
            egl_swap_failures: AtomicU64::new(0),
            mpv_report_swap_calls: AtomicU64::new(0),
            wake_events: AtomicU64::new(0),
            redraw_requests: AtomicU64::new(0),
            no_frame_fallback_deadlines: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
            decoder_dropped: AtomicI64::new(UNAVAILABLE_INT),
            presentation_feedback_available: AtomicBool::new(false),
            presentation_feedback_total: AtomicU64::new(0),
            snapshot: Mutex::new(initial),
            timing: Mutex::new(TimingSamples::default()),
            rows_written: AtomicU64::new(0),
            paint_seq: AtomicU64::new(0),
            deadline_expired: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            publish_skips: AtomicU64::new(0),
        }
    }

    /// Whether the current state expects the pipeline to keep producing frames.
    ///
    /// `Stalled` is included deliberately: a pipeline that stopped progressing
    /// unexpectedly should be retried, not abandoned. Rate limiting keeps the
    /// retry cheap. Intentional states (paused/EOF/idle, and a suspension the
    /// windowing layer is responsible for) stay quiet.
    pub fn expects_progress(&self) -> bool {
        match self.snapshot.lock() {
            Ok(s) => matches!(
                s.state,
                crate::presentation::PresentationState::Presenting
                    | crate::presentation::PresentationState::AwaitingFrame
                    | crate::presentation::PresentationState::Stalled
            ),
            Err(_) => false,
        }
    }

    /// Records that a paint happened (cheap; called once per frame).
    pub fn note_paint(&self) {
        self.paint_seq.fetch_add(1, Ordering::Relaxed);
    }

    pub fn paint_seq(&self) -> u64 {
        self.paint_seq.load(Ordering::Relaxed)
    }

    /// Counts one wake event **delivered** to the event loop.
    ///
    /// This is the only increment site, and the CSV reads this atomic, so the
    /// per-second rows and the teardown summary cannot disagree (Part 6).
    pub fn note_wake_delivered(&self) {
        self.wake_events.fetch_add(1, Ordering::Relaxed);
    }

    /// Counts one redraw **request** issued by the application.
    pub fn note_redraw_request(&self) {
        self.redraw_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn wake_events(&self) -> u64 {
        self.wake_events.load(Ordering::Relaxed)
    }

    pub fn redraw_requests(&self) -> u64 {
        self.redraw_requests.load(Ordering::Relaxed)
    }

    /// Marks the run deadline as passed (idempotent; returns `true` once).
    pub fn mark_deadline_expired(&self) -> bool {
        !self.deadline_expired.swap(true, Ordering::SeqCst)
    }

    pub fn deadline_expired(&self) -> bool {
        self.deadline_expired.load(Ordering::SeqCst)
    }

    /// Publishes counters and the presentation snapshot. Never blocks.
    ///
    /// Kept for the once-per-second counter cadence and for teardown; the
    /// snapshot is also published on its own (see [`Self::publish_snapshot`]).
    pub fn publish(&self, counters: &Counters, snapshot: PresentationSnapshot) {
        store(&self.update_callbacks, counters.update_callbacks_total);
        store(&self.new_video_frames, counters.new_video_frames_total);
        store(&self.fbo_render_calls, counters.fbo_render_calls_total);
        store(&self.egl_swap_attempts, counters.egl_swap_attempts_total);
        store(&self.egl_swap_successes, counters.egl_swap_successes_total);
        store(&self.egl_swap_failures, counters.egl_swap_failures_total);
        store(
            &self.mpv_report_swap_calls,
            counters.mpv_report_swap_calls_total,
        );
        store(&self.wake_events, counters.wake_events_total);
        store(&self.redraw_requests, counters.redraw_requests_total);
        store(
            &self.no_frame_fallback_deadlines,
            counters.no_frame_fallback_deadlines_total,
        );
        store(&self.dropped_frames, counters.dropped_frames_total);
        self.decoder_dropped
            .store(counters.decoder_dropped_total, Ordering::Relaxed);

        self.publish_snapshot(snapshot);
    }

    /// Publishes only the presentation snapshot. Never blocks.
    ///
    /// Separate from [`Self::publish`] because the two publications have
    /// different costs and different freshness requirements. The counter set is
    /// gated to once per second because reading it costs mpv property reads;
    /// the snapshot is a plain `Copy` with no runtime access at all, and its
    /// timestamps are the *only* input to `last_present_age_s`,
    /// `last_swap_age_s` and `last_callback_age_s`.
    ///
    /// Bundled into the once-per-second publish, a telemetry sample taken just
    /// before a publish measured those ages from the *previous* publish, up to
    /// a full second earlier. WP1 preflight-01 recorded exactly that at t=3.012:
    /// `last_present_age_s=1.002` with 60 successful EGL swaps in the same
    /// second, because the sample read the snapshot published at t=2.010 and
    /// compared it against the sampling instant. Ages therefore have to be
    /// published per frame, not per second.
    pub fn publish_snapshot(&self, snapshot: PresentationSnapshot) {
        match self.snapshot.try_lock() {
            Ok(mut slot) => *slot = snapshot,
            Err(_) => {
                self.publish_skips.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Reads the counters. Called only by the telemetry thread.
    pub fn counters(&self) -> Counters {
        Counters {
            update_callbacks_total: self.update_callbacks.load(Ordering::Relaxed),
            new_video_frames_total: self.new_video_frames.load(Ordering::Relaxed),
            fbo_render_calls_total: self.fbo_render_calls.load(Ordering::Relaxed),
            egl_swap_attempts_total: self.egl_swap_attempts.load(Ordering::Relaxed),
            egl_swap_successes_total: self.egl_swap_successes.load(Ordering::Relaxed),
            egl_swap_failures_total: self.egl_swap_failures.load(Ordering::Relaxed),
            mpv_report_swap_calls_total: self.mpv_report_swap_calls.load(Ordering::Relaxed),
            wake_events_total: self.wake_events.load(Ordering::Relaxed),
            redraw_requests_total: self.redraw_requests.load(Ordering::Relaxed),
            no_frame_fallback_deadlines_total: self
                .no_frame_fallback_deadlines
                .load(Ordering::Relaxed),
            dropped_frames_total: self.dropped_frames.load(Ordering::Relaxed),
            decoder_dropped_total: self.decoder_dropped.load(Ordering::Relaxed),
        }
    }

    /// Latest published snapshot. Waits only if the render thread is mid-store.
    pub fn snapshot(&self) -> PresentationSnapshot {
        *self.snapshot.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Records one successful present's timings. Never blocks.
    pub fn note_present(&self, now: Instant, swap_call_us: u64) {
        let Ok(mut t) = self.timing.try_lock() else {
            // Another thread holds the buffer for microseconds; drop this
            // sample rather than stall the renderer waiting for it.
            return;
        };
        if let Some(last) = t.frame_interval_us.last() {
            // The buffers hold deltas; the caller supplies absolute instants in
            // `record_gap` instead. Kept for API safety.
            let _ = last;
        }
        if !TimingSamples::push(&mut t.swap_call_us, swap_call_us) {
            t.dropped_samples = t.dropped_samples.saturating_add(1);
        }
        let _ = now;
    }

    /// Records the gap between two consecutive successful presents.
    ///
    /// This is the *presentation* cadence: the interval between two buffers
    /// handed to the compositor, not the interval between two decoded video
    /// frames. It feeds both `inter_swap_gap_*` and `frame_interval_*`; the two
    /// histograms are therefore the same event under two names, and
    /// `frame_interval_*` is compared against `target_frame_interval_ms`, which
    /// is the display period (60 Hz by default), never the source frame rate.
    /// At 24 fps source into a 60 Hz surface the expected value is ~16.7 ms, not
    /// 41.7 ms, and that is why the two columns match in every row.
    pub fn note_present_gap(&self, gap_us: u64) {
        let Ok(mut t) = self.timing.try_lock() else {
            return;
        };
        t.longest_gap_ms = t.longest_gap_ms.max(gap_us as f64 / 1000.0);
        if !TimingSamples::push(&mut t.inter_swap_gap_us, gap_us)
            || !TimingSamples::push(&mut t.frame_interval_us, gap_us)
        {
            t.dropped_samples = t.dropped_samples.saturating_add(1);
        }
    }

    fn take_timing(&self) -> TimingSamples {
        let mut t = self.timing.lock().unwrap_or_else(|e| e.into_inner());
        t.take()
    }

    pub fn rows_written(&self) -> u64 {
        self.rows_written.load(Ordering::Relaxed)
    }

    /// Marks whether this run has a compositor presentation-feedback source.
    pub fn set_presentation_feedback_available(&self, available: bool) {
        self.presentation_feedback_available
            .store(available, Ordering::Release);
    }

    /// Counts one `wp_presentation_feedback.presented` event.
    pub fn note_presented_feedback(&self) {
        self.presentation_feedback_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// `Some(count)` only when the compositor feedback protocol is active.
    pub fn presentation_feedback_total(&self) -> Option<u64> {
        self.presentation_feedback_available
            .load(Ordering::Acquire)
            .then(|| self.presentation_feedback_total.load(Ordering::Relaxed))
    }

    pub fn publish_skips(&self) -> u64 {
        self.publish_skips.load(Ordering::Relaxed)
    }

    pub fn request_stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
    }

    fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }
}

/// Configuration for [`TelemetryWorker`].
#[derive(Clone)]
pub struct TelemetryConfig {
    pub run: String,
    pub path: Option<String>,
    pub target_frame_interval: Duration,
    pub started: Instant,
    /// Optional nudge channel used only to break a parked event loop (see the
    /// worker's liveness check). The worker never touches EGL, GL or libmpv.
    pub notify: Option<Arc<dyn crate::wake::WakeHandle>>,
    /// Stall threshold used for the worker's time-aware state classification.
    /// Must match the state machine's so the two never disagree about an
    /// episode boundary.
    pub stall_threshold: Duration,
    /// Run length from `MUVRIX_RUN_SECONDS`. Enforced here as well as in the
    /// painter, so a bounded run terminates even with no render at all.
    pub run_limit: Option<Duration>,
}

impl TelemetryConfig {
    /// Reads the run name and output path from the environment, which is how the
    /// harness configures a measured run.
    pub fn from_env(target_frame_interval: Duration, started: Instant) -> Self {
        Self {
            run: std::env::var("MUVRIX_RUN_NAME").unwrap_or_else(|_| "runner".to_string()),
            path: std::env::var("MUVRIX_RUNNER_TELEMETRY").ok(),
            target_frame_interval,
            started,
            notify: None,
            stall_threshold: crate::presentation::DEFAULT_STALL_THRESHOLD,
            run_limit: None,
        }
    }

    /// Sets the deadline the run must terminate by (monotonic, from `started`).
    pub fn with_run_limit(mut self, limit: Option<Duration>) -> Self {
        self.run_limit = limit;
        self
    }

    pub fn with_stall_threshold(mut self, threshold: Duration) -> Self {
        self.stall_threshold = threshold;
        self
    }

    /// Arms the liveness nudge.
    pub fn with_notify(mut self, notify: Arc<dyn crate::wake::WakeHandle>) -> Self {
        self.notify = Some(notify);
        self
    }
}

/// Independent per-second emitter.
///
/// Owns its thread and its writer. The thread sleeps in fixed small steps and
/// emits whenever a whole second has elapsed *in wall/monotonic time*, so rows
/// keep appearing while the render thread is blocked, the event loop is stalled,
/// or the compositor is withholding frames.
pub struct TelemetryWorker {
    handle: Option<JoinHandle<()>>,
    metrics: Arc<SharedMetrics>,
}

impl TelemetryWorker {
    pub fn spawn(metrics: Arc<SharedMetrics>, config: TelemetryConfig) -> Self {
        let worker_metrics = metrics.clone();
        let handle = std::thread::Builder::new()
            .name("muvrix-telemetry".to_string())
            .spawn(move || run_worker(worker_metrics, config))
            .ok();
        if handle.is_none() {
            log::error!("telemetry worker could not start; CSV will not be written");
        }
        Self { handle, metrics }
    }

    pub fn metrics(&self) -> &Arc<SharedMetrics> {
        &self.metrics
    }

    pub fn rows_written(&self) -> u64 {
        self.metrics.rows_written()
    }

    /// Stops the thread and waits for it. Safe to call once; later calls are
    /// no-ops.
    pub fn shutdown(&mut self) {
        self.metrics.request_stop();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for TelemetryWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_worker(metrics: Arc<SharedMetrics>, config: TelemetryConfig) {
    let mut writer = match config.path.as_deref() {
        Some(p) => match TelemetryWriter::create(p) {
            Ok(w) => {
                log::info!(
                    "telemetry worker: {} fields/row -> {}",
                    CANONICAL_FIELD_COUNT,
                    p
                );
                Some(w)
            }
            Err(e) => {
                log::warn!("cannot create telemetry {p}: {e}");
                None
            }
        },
        None => None,
    };

    let mut last_report = config.started;
    let mut prev = Counters::default();
    // The worker polls faster than once per second so rows land close to the
    // boundary even when the render thread is silent.
    let poll = Duration::from_millis(50);

    // Liveness: mpv blocks waiting for a swap report, and if the event loop parks
    // with no deadline armed nothing asks for the paint that would produce it.
    // The worker notices that paints stopped while progress is expected and asks
    // for one repaint. It cannot deadlock anything: the request is a non-blocking
    // proxy send, and it is rate-limited.
    let mut paint_seq = metrics.paint_seq();
    let mut last_paint_change = Instant::now();
    let mut nudges: u64 = 0;
    let stall_after = Duration::from_millis(400);

    // Phase 2.5: the worker owns the time-aware stall classification, because
    // it is the only component that keeps running when the painter does not.
    // Episode bookkeeping lives here so exactly one stall/recovery pair is
    // emitted per episode even though every row recomputes the state.
    let mut in_stall = false;
    let mut deadline_logged = false;

    while !metrics.stopping() {
        std::thread::sleep(poll);
        let now = Instant::now();

        // --- Run deadline, independent of rendering (Part 7).
        if let Some(limit) = config.run_limit {
            if !deadline_logged && now.saturating_duration_since(config.started) >= limit {
                deadline_logged = true;
                let expired = metrics.mark_deadline_expired();
                log::info!(
                    "run deadline expired ({limit:?}) on the telemetry thread; requesting clean exit"
                );
                crate::trace::record(
                    crate::trace::Ev::RunDeadline,
                    "worker",
                    limit.as_millis() as i64,
                    expired as i64,
                );
                if let Some(notify) = &config.notify {
                    // Wakes the loop even if it is parked with no deadline, and
                    // is harmless if the painter is mid-frame.
                    notify.set_deadline(Instant::now());
                }
            }
        }

        let seq = metrics.paint_seq();
        if seq != paint_seq {
            paint_seq = seq;
            last_paint_change = now;
        } else if now.saturating_duration_since(last_paint_change) >= stall_after
            && metrics.expects_progress()
        {
            if let Some(notify) = &config.notify {
                // A deadline that is already due is exactly "repaint now", and it
                // goes through the same merged/earliest-deadline path as every
                // other wake request.
                notify.set_deadline(Instant::now());
                nudges = nudges.saturating_add(1);
                if nudges <= 3 || nudges.is_multiple_of(30) {
                    log::warn!(
                        "no paint for {}ms while progress expected: requested a repaint ({nudges})",
                        now.saturating_duration_since(last_paint_change).as_millis()
                    );
                }
            }
            last_paint_change = now;
        }

        if now.saturating_duration_since(last_report) < Duration::from_secs(1) {
            continue;
        }

        let counters = metrics.counters();
        let raw_snapshot = metrics.snapshot();
        let snapshot = raw_snapshot.with_time_aware_state(now, config.stall_threshold);
        let progress_expected = matches!(
            raw_snapshot.state,
            crate::presentation::PresentationState::Presenting
                | crate::presentation::PresentationState::AwaitingFrame
        );

        // Episode accounting: one start and one recovery per episode, no matter
        // how many rows are written while it is open.
        if progress_expected && snapshot.state == crate::presentation::PresentationState::Stalled {
            if !in_stall {
                in_stall = true;
                let age_ms = raw_snapshot
                    .last_present_at
                    .map(|at| now.saturating_duration_since(at).as_millis() as u64)
                    .unwrap_or(0);
                log::warn!(
                    "presentation stalled after {age_ms}ms without a successful swap (classified by the telemetry thread)"
                );
                crate::trace::record(
                    crate::trace::Ev::StallStart,
                    "worker",
                    age_ms as i64,
                    config.stall_threshold.as_millis() as i64,
                );
                crate::trace::dump("stall_start");
            }
        } else if in_stall && snapshot.state == crate::presentation::PresentationState::Presenting {
            in_stall = false;
            let age_ms = raw_snapshot
                .last_present_at
                .map(|at| now.saturating_duration_since(at).as_millis() as u64)
                .unwrap_or(0);
            log::info!("presentation recovered {age_ms}ms after the last successful swap");
            crate::trace::record(crate::trace::Ev::StallRecovery, "worker", age_ms as i64, 0);
            crate::trace::dump("stall_recovery");
        } else if in_stall && !progress_expected {
            // Paused/EOF/idle arrived while the episode was open: it ended by an
            // intentional state, not by a presentation, so it is not a recovery.
            in_stall = false;
            crate::trace::record(
                crate::trace::Ev::StallRecovery,
                "closed_by_intentional_state",
                0,
                0,
            );
        }

        let timing = metrics.take_timing();
        let clock = ClockSample::now();
        let row = TelemetryRow::sample(
            now,
            config.started,
            &config.run,
            config.target_frame_interval,
            counters,
            &prev,
            snapshot,
            &timing,
            clock,
            metrics.presentation_feedback_total(),
        );

        log::info!(
            "telemetry: t={:.1}s state={} state_age={:.2}s last_present={} \
             new_frames/s={} report_swap/s={} egl_ok/s={} egl_fail/s={} \
             callbacks/s={} wake/s={} redraw/s={} fallback/s={} max_gap={:.2}ms \
             episodes={} recoveries={}",
            row.t_s,
            row.state,
            row.ages.state_age_s,
            fmt_opt(row.ages.last_present_age_s),
            row.delta.new_video_frames,
            row.delta.mpv_report_swap_calls,
            row.delta.egl_swap_successes,
            row.delta.egl_swap_failures,
            row.delta.update_callbacks,
            row.delta.wake_events,
            row.delta.redraw_requests,
            row.delta.no_frame_fallback_deadlines,
            timing.longest_gap_ms,
            row.snapshot.unavailable_episodes,
            row.snapshot.unavailable_recoveries
        );

        if let Some(w) = writer.as_mut() {
            if w.write_row(&row) {
                metrics.rows_written.fetch_add(1, Ordering::Relaxed);
            } else {
                log::error!("telemetry row rejected; keeping the file valid");
            }
        } else {
            metrics.rows_written.fetch_add(1, Ordering::Relaxed);
        }

        prev = counters;
        last_report = now;
    }

    // --- Shutdown: one reconciliation block (Part 6) and a final trace dump.
    //
    // `prev` is the counter set of the last row written; `metrics.counters()` is
    // the same atomics now. Any difference is work that happened after the last
    // second boundary, which is bounded and explainable — the point of the block
    // is that both numbers come from one source and the difference is stated.
    let fin = metrics.counters();
    let wake_delta = fin.wake_events_total.saturating_sub(prev.wake_events_total);
    let redraw_delta = fin
        .redraw_requests_total
        .saturating_sub(prev.redraw_requests_total);
    log::info!(
        "counter reconciliation: rows={} wake_events csv={} final={} delta={} \
         redraw_requests csv={} final={} delta={} nudges={} deadline_expired={}",
        metrics.rows_written(),
        prev.wake_events_total,
        fin.wake_events_total,
        wake_delta,
        prev.redraw_requests_total,
        fin.redraw_requests_total,
        redraw_delta,
        nudges,
        metrics.deadline_expired()
    );
    crate::trace::record(
        crate::trace::Ev::Teardown,
        "worker",
        prev.wake_events_total as i64,
        fin.wake_events_total as i64,
    );
    crate::trace::dump("teardown");
}

fn exceedances(samples_us: &[u64], target_ms: f64) -> (u64, u64, u64) {
    let target_us = target_ms * 1000.0;
    let mut x15 = 0;
    let mut x2 = 0;
    let mut x3 = 0;
    for &s in samples_us {
        let s = s as f64;
        if s > target_us * 3.0 {
            x3 += 1;
            x2 += 1;
            x15 += 1;
        } else if s > target_us * 2.0 {
            x2 += 1;
            x15 += 1;
        } else if s > target_us * 1.5 {
            x15 += 1;
        }
    }
    (x15, x2, x3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::{PresentationState, StateEvent};

    /// Phase 2.5: a *stalled* pipeline must keep being retried; only states where
    /// no progress is expected may stay quiet. Losing this distinction is what let
    /// a test stall sit unresolved until the end of the run.
    #[test]
    fn stalled_and_presenting_expect_progress_but_suspension_does_not() {
        use crate::presentation::PresentationSnapshot;
        for (state, expected) in [
            (PresentationState::Presenting, true),
            (PresentationState::AwaitingFrame, true),
            (PresentationState::Stalled, true),
            (PresentationState::NoFrameExpected, false),
            (PresentationState::SurfaceUnavailable, false),
            (PresentationState::DeviceLost, false),
            (PresentationState::Failed, false),
        ] {
            let snapshot = PresentationSnapshot {
                state,
                state_since: Instant::now(),
                last_present_at: None,
                last_swap_at: None,
                last_callback_at: None,
                unavailable_episodes: 0,
                unavailable_recoveries: 0,
                last_event_t_s: 0.0,
            };
            let metrics = SharedMetrics::new(snapshot);
            assert_eq!(
                metrics.expects_progress(),
                expected,
                "unexpected liveness policy for {state:?}"
            );
        }
    }

    #[test]
    fn header_has_exactly_56_fields() {
        assert_eq!(header_field_count(), CANONICAL_FIELD_COUNT);
        assert_eq!(split_fields(CANONICAL_HEADER).len(), 56);
        // Spot-check both ends and a middle field so a reordering is caught.
        let f = split_fields(CANONICAL_HEADER);
        assert_eq!(f[0], "timestamp");
        assert_eq!(f[4], "state");
        assert_eq!(f[20], "egl_swap_successes_this_second");
        assert_eq!(f[26], "wayland_presented_feedback_total");
        assert_eq!(f[55], "notes");
    }

    fn machine_at(now: Instant) -> PresentationStateMachine {
        let mut m = PresentationStateMachine::new(now);
        m.on_event(StateEvent::SwapSucceeded, now);
        m
    }

    fn sample_row_at(now: Instant) -> TelemetryRow {
        let m = machine_at(now);
        TelemetryRow::sample(
            now + Duration::from_millis(20),
            now,
            "unit",
            Duration::from_micros(16_667),
            Counters::default(),
            &Counters::default(),
            m.snapshot(),
            &TimingSamples::default(),
            ClockSample {
                monotonic_s: Some(1.0),
                boottime_s: Some(2.0),
                realtime_s: Some(3.0),
                boottime_minus_monotonic_s: Some(1.0),
            },
            None,
        )
    }

    #[test]
    fn row_serializes_to_56_fields() {
        let line = sample_row_at(Instant::now()).to_csv_line();
        assert_eq!(field_count(&line), CANONICAL_FIELD_COUNT, "line: {line}");
        assert!(validate_row(&line).is_ok());
    }

    #[test]
    fn validator_accepts_56_and_rejects_55_and_57() {
        let line = sample_row_at(Instant::now()).to_csv_line();
        let f = split_fields(&line);
        let short = f[..55].join(",");
        let long = format!("{line},extra");
        assert!(validate_row(&line).is_ok());
        assert_eq!(
            validate_row(&short).unwrap_err(),
            FieldCountDefect {
                expected: 56,
                actual: 55
            }
        );
        assert_eq!(
            validate_row(&long).unwrap_err(),
            FieldCountDefect {
                expected: 56,
                actual: 57
            }
        );
    }

    #[test]
    fn ages_are_measured_from_event_time_not_sample_time() {
        // This is the Phase 2.3 defect: the sample and the event shared one
        // `Instant`, so `last_present_age_s` was structurally 0.000 forever and
        // could not show a stall.
        let now = Instant::now();
        let m = machine_at(now);
        let later = now + Duration::from_millis(750);
        let row = TelemetryRow::sample(
            later,
            now,
            "unit",
            Duration::from_micros(16_667),
            Counters::default(),
            &Counters::default(),
            m.snapshot(),
            &TimingSamples::default(),
            ClockSample::now(),
            None,
        );
        let age = row.ages.last_present_age_s.unwrap();
        assert!((age - 0.75).abs() < 0.001, "age = {age}");
        assert!((row.ages.last_swap_age_s.unwrap() - 0.75).abs() < 0.001);
        // No callback has ever been observed: unavailable, not zero.
        assert!(row.ages.last_callback_age_s.is_none());
        let line = row.to_csv_line();
        let f = split_fields(&line);
        assert_eq!(f[6], "0.750");
        assert_eq!(f[8], "-1");
    }

    #[test]
    fn unavailable_values_print_as_minus_one() {
        let mut row = sample_row_at(Instant::now());
        row.clock.boottime_s = None;
        row.clock.boottime_minus_monotonic_s = None;
        row.swap_call = None;
        let line = row.to_csv_line();
        let f = split_fields(&line);
        assert_eq!(f[26], "-1", "feedback must be unavailable");
        assert_eq!(f[27], "-1", "missing percentiles must not print 0");
        assert_eq!(f[52], "-1", "missing clock must not print 0");
        assert_eq!(field_count(&line), CANONICAL_FIELD_COUNT);
    }

    #[test]
    fn notes_commas_do_not_break_the_row() {
        let mut row = sample_row_at(Instant::now());
        row.notes = "stall, watchdog=7\nsecond line".into();
        let line = row.to_csv_line();
        assert_eq!(field_count(&line), CANONICAL_FIELD_COUNT);
        assert!(line.ends_with("stall; watchdog=7;second line"));
    }

    #[test]
    fn percentiles_are_nearest_rank() {
        let samples: Vec<u64> = (1..=100).map(|v| v * 1000).collect();
        let p = percentiles(&samples).unwrap();
        assert_eq!(p.count, 100);
        assert!((p.p50 - 50.0).abs() < 1e-9);
        assert!((p.p95 - 95.0).abs() < 1e-9);
        assert!((p.p99 - 99.0).abs() < 1e-9);
        assert!((p.max - 100.0).abs() < 1e-9);
        assert!(percentiles(&[]).is_none());
    }

    #[test]
    fn iso8601_epoch_and_known_date() {
        assert_eq!(iso8601_utc(UNIX_EPOCH), "1970-01-01T00:00:00Z".to_string());
        let t = UNIX_EPOCH + Duration::from_secs(1_763_000_000);
        let s = iso8601_utc(t);
        assert!(s.starts_with("2025-") || s.starts_with("2026-"), "{s}");
        assert!(s.ends_with('Z'));
        assert_eq!(s.len(), 20);
    }

    #[test]
    fn deltas_are_per_second_not_cumulative() {
        let prev = Counters {
            new_video_frames_total: 100,
            egl_swap_successes_total: 400,
            ..Counters::default()
        };
        let now = Counters {
            new_video_frames_total: 124,
            egl_swap_successes_total: 460,
            ..Counters::default()
        };
        let d = CounterDeltas::between(&prev, &now);
        assert_eq!(d.new_video_frames, 24);
        assert_eq!(d.egl_swap_successes, 60);
    }

    #[test]
    fn exceedance_buckets_use_target_interval() {
        // Target 16 ms → 1.5x=24, 2x=32, 3x=48 (ms).
        let samples = vec![16_000, 25_000, 33_000, 50_000];
        let (x15, x2, x3) = exceedances(&samples, 16.0);
        assert_eq!(x3, 1); // 50
        assert_eq!(x2, 2); // 33, 50
        assert_eq!(x15, 3); // 25, 33, 50
    }

    #[test]
    fn writer_refuses_malformed_rows() {
        let dir =
            std::env::temp_dir().join(format!("muvrix-telemetry-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("run.csv");
        let mut writer = TelemetryWriter::create(&path).unwrap();

        let good = sample_row_at(Instant::now()).to_csv_line();
        assert!(writer.write_line(&good).is_ok());
        let short = split_fields(&good)[..55].join(",");
        assert!(writer.write_line(&short).is_err());
        let long = format!("{good},extra");
        assert!(writer.write_line(&long).is_err());

        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();
        assert_eq!(field_count(lines.next().unwrap()), 56);
        assert_eq!(field_count(lines.next().unwrap()), 56);
        assert!(lines.next().is_none(), "malformed rows must not be written");
        assert_eq!(writer.lines(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_numeric_field_is_detectable() {
        let line = sample_row_at(Instant::now()).to_csv_line();
        let mut f: Vec<String> = split_fields(&line).into_iter().map(String::from).collect();
        f[2] = "not-a-number".into();
        let bad = f.join(",");
        assert_eq!(field_count(&bad), CANONICAL_FIELD_COUNT);
        assert!(split_fields(&bad)[2].parse::<f64>().is_err());
    }

    #[test]
    fn worker_emits_rows_without_any_rendering_activity() {
        // The whole point: rows must appear while nothing publishes frames.
        let dir = std::env::temp_dir().join(format!("muvrix-worker-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("run.csv");

        let start = Instant::now();
        let m = machine_at(start);
        let metrics = Arc::new(SharedMetrics::new(m.snapshot()));
        metrics.publish(&Counters::default(), m.snapshot());
        let mut worker = TelemetryWorker::spawn(
            metrics.clone(),
            TelemetryConfig {
                run: "worker-unit".into(),
                path: Some(path.to_string_lossy().into_owned()),
                target_frame_interval: Duration::from_micros(16_667),
                started: start,
                notify: None,
                stall_threshold: Duration::from_millis(500),
                run_limit: None,
            },
        );

        // Sleep through two row boundaries, publishing nothing at all.
        std::thread::sleep(Duration::from_millis(2_300));
        let rows_while_idle = metrics.rows_written();
        assert!(
            rows_while_idle >= 2,
            "worker wrote {rows_while_idle} rows with no rendering activity"
        );

        worker.shutdown();
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(field_count(lines[0]), 56);
        for line in &lines[1..] {
            assert_eq!(field_count(line), 56, "line: {line}");
            assert!(line.contains("worker-unit"));
        }
        // The idle rows must show a growing presentation age, not a frozen zero.
        let last = lines.last().unwrap();
        let f = split_fields(last);
        let age: f64 = f[6].parse().unwrap();
        assert!(age > 1.0, "expected a real presentation age, got {age}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn publish_never_blocks_when_the_snapshot_lock_is_held() {
        let start = Instant::now();
        let m = machine_at(start);
        let metrics = Arc::new(SharedMetrics::new(m.snapshot()));
        let held = metrics.snapshot.lock().unwrap();
        metrics.publish(&Counters::default(), m.snapshot());
        assert_eq!(metrics.publish_skips(), 1, "publish must skip, not wait");
        drop(held);
        metrics.publish(&Counters::default(), m.snapshot());
        assert_eq!(metrics.snapshot().state, PresentationState::Presenting);
    }

    // --- WP1 diagnostic: snapshot freshness versus the counter cadence -------

    fn snapshot_at(
        state: PresentationState,
        state_since: Instant,
        presented_at: Option<Instant>,
    ) -> PresentationSnapshot {
        PresentationSnapshot {
            state,
            state_since,
            last_present_at: presented_at,
            last_swap_at: presented_at,
            last_callback_at: presented_at,
            unavailable_episodes: 0,
            unavailable_recoveries: 0,
            last_event_t_s: 0.0,
        }
    }

    #[test]
    fn a_successful_present_refreshes_the_age_without_a_counter_publish() {
        // The counter set is published once per second (five mpv property reads);
        // the presentation snapshot must not inherit that cadence, because its
        // timestamps are the only input to `last_present_age_s`.
        let base = Instant::now();
        let mut m = PresentationStateMachine::new(base);
        m.on_event(StateEvent::PlaybackStarted, base);
        m.on_event(StateEvent::SwapSucceeded, base + Duration::from_millis(500));

        let metrics = SharedMetrics::new(m.snapshot());
        metrics.publish(&Counters::default(), m.snapshot());

        let sample_at = base + Duration::from_millis(1_490);
        m.on_event(StateEvent::SwapSucceeded, sample_at);
        // No counter publication happens here — only the snapshot.
        metrics.publish_snapshot(m.snapshot());

        let ages = PresentationStateMachine::snapshot_ages(&metrics.snapshot(), sample_at);
        assert!(
            ages.last_present_age_s.unwrap() < 0.001,
            "a present must refresh the age immediately, got {:?}",
            ages.last_present_age_s
        );
    }

    #[test]
    fn fresh_presentation_is_not_stalled_by_a_stale_counter_window() {
        // Regression for preflight-01 t=3.012: `last_present_age_s=1.002` while
        // 60 EGL swaps succeeded in that same second. The sample read the
        // snapshot published at t=2.010 and measured the age from the sampling
        // instant, so a healthy renderer crossed the 1000 ms stall threshold
        // purely because of where the sample landed between publishes.
        let base = Instant::now();
        let threshold = Duration::from_millis(1_000);
        let published = snapshot_at(
            PresentationState::Presenting,
            base + Duration::from_millis(375),
            Some(base + Duration::from_millis(2_010)),
        );
        let sample_at = base + Duration::from_millis(3_012);

        // The pre-fix view: a snapshot a full second older than the sample.
        assert_eq!(
            published.with_time_aware_state(sample_at, threshold).state,
            PresentationState::Stalled,
            "a 1.002 s old snapshot must still classify as stalled"
        );

        // The fix: the same sample, but the snapshot comes from the present that
        // just succeeded. The counter window is unchanged and stale; freshness of
        // the authoritative presentation event must win.
        let metrics = SharedMetrics::new(published);
        let fresh = snapshot_at(
            PresentationState::Presenting,
            base + Duration::from_millis(375),
            Some(base + Duration::from_millis(3_010)),
        );
        metrics.publish_snapshot(fresh);
        let counters = metrics.counters();
        assert_eq!(
            (
                counters.new_video_frames_total,
                counters.egl_swap_successes_total
            ),
            (0, 0),
            "counters must still be the stale ones"
        );
        assert_eq!(
            metrics
                .snapshot()
                .with_time_aware_state(sample_at, threshold)
                .state,
            PresentationState::Presenting
        );
    }

    #[test]
    fn per_second_deltas_reconcile_with_cumulative_totals() {
        // Every row's `*_this_second` must be the difference of two cumulative
        // totals, so summing the rows reproduces the last total exactly.
        let publishes = [
            Counters {
                new_video_frames_total: 24,
                egl_swap_successes_total: 60,
                ..Counters::default()
            },
            Counters {
                new_video_frames_total: 72,
                egl_swap_successes_total: 178,
                ..Counters::default()
            },
            Counters {
                new_video_frames_total: 96,
                egl_swap_successes_total: 238,
                ..Counters::default()
            },
        ];
        let mut prev = Counters::default();
        let mut frames = 0u64;
        let mut swaps = 0u64;
        for now in &publishes {
            let d = CounterDeltas::between(&prev, now);
            assert_reconciles(&prev, now, &d);
            frames += d.new_video_frames;
            swaps += d.egl_swap_successes;
            prev = *now;
        }
        let last = publishes.last().unwrap();
        assert_eq!(frames, last.new_video_frames_total);
        assert_eq!(swaps, last.egl_swap_successes_total);
    }

    /// A delta must be exactly what turns the previous total into the next one.
    fn assert_reconciles(prev: &Counters, now: &Counters, d: &CounterDeltas) {
        assert_eq!(
            prev.new_video_frames_total + d.new_video_frames,
            now.new_video_frames_total
        );
        assert_eq!(
            prev.egl_swap_successes_total + d.egl_swap_successes,
            now.egl_swap_successes_total
        );
    }
}

#[cfg(test)]
mod schema_document_tests {
    use super::*;

    /// The schema document is the published contract; the constant is what the
    /// writer actually emits. They must not drift. The document lives outside
    /// the crate (and under an ignored path), so a checkout without it skips
    /// this test rather than failing.
    #[test]
    fn schema_document_header_matches_the_code_header() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/phase-2.2/artifacts/telemetry-csv-schema.md"
        );
        let Ok(doc) = std::fs::read_to_string(path) else {
            eprintln!("schema document not present; drift check skipped");
            return;
        };
        let Some(block) = doc.split("```text").nth(1) else {
            panic!("schema document has no ```text header block");
        };
        let Some(block) = block.split("```").next() else {
            panic!("schema document header block is unterminated");
        };
        let documented: String = block.chars().filter(|c| *c != '\n').collect();
        assert_eq!(documented.trim(), CANONICAL_HEADER);
        assert_eq!(field_count(&documented), CANONICAL_FIELD_COUNT);
    }
}
