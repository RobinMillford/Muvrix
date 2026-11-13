//! Paint-independent mpv playback-state sampler (Phase 2.6 WP1 recovery).
//!
//! ## Why this exists
//!
//! Every pre-existing telemetry path is gated behind `draw()`: the once-per-
//! second emitter reads counters that only the painter publishes, and mpv
//! events are polled from `MuvrixApp::frame_update`. When the compositor
//! withholds frame callbacks, the painter stops, the counters freeze, and the
//! question "did playback stop or did only rendering stop?" becomes
//! unanswerable — which is exactly what happened in Run-01.
//!
//! This sampler reads a small set of scalar mpv properties from its own thread
//! and appends one timestamped CSV row per interval. It never calls `draw()`,
//! never touches EGL/GL/egui, and never calls `mpv_wait_event`, so it does not
//! compete with the controller's single event consumer.
//!
//! ## Threading
//!
//! `MpvHandle` is `Send + Sync` because upstream `client.h` guarantees that a
//! client handle is thread-safe for API operations; `mpv_get_property` may be
//! called from any thread. `mpv_wait_event` must not be called concurrently,
//! and this module does not call it.
//!
//! The sampler is stopped and joined by `Shell::destroy` before the mpv handle
//! is torn down, so no property call can race termination.
//!
//! ## Output
//!
//! Enabled only when `MUVRIX_MPV_STATE_CSV` names a path. Off by default so an
//! ordinary run writes no extra file. Unavailable properties are written as
//! `-1`, never invented.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use muvrix_player::MpvHandle;

use crate::telemetry::iso8601_utc;

/// Sampling cadence. Two rows per second is enough to show whether the media
/// clock advanced during a multi-second rendering interruption, and costs six
/// property reads per sample.
pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

/// How often the worker checks its stop flag between samples.
const STOP_POLL: Duration = Duration::from_millis(50);

/// CSV header, one physical line.
pub const CANONICAL_HEADER: &str =
    "timestamp_utc,t_s,pause,playback_time_s,eof_reached,core_idle,idle_active,demuxer_cache_state,notes";

/// Field count of [`CANONICAL_HEADER`].
pub const CANONICAL_FIELD_COUNT: usize = 9;

const UNAVAILABLE: &str = "-1";
const MAX_CACHE_STATE_LEN: usize = 240;

/// One sampled mpv state point. Every field is optional: an unavailable
/// property must remain visibly unavailable rather than defaulting to a
/// plausible-looking value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MpvStateSample {
    pub pause: Option<bool>,
    pub playback_time: Option<f64>,
    pub eof_reached: Option<bool>,
    pub core_idle: Option<bool>,
    pub idle_active: Option<bool>,
    pub demuxer_cache_state: Option<String>,
}

impl MpvStateSample {
    /// Reads the properties from a live handle. Never panics; a failed read is
    /// recorded as `None`.
    pub fn read(handle: &MpvHandle) -> Self {
        Self {
            pause: handle.get_property::<bool>("pause").ok(),
            playback_time: handle.get_property::<f64>("playback-time").ok(),
            eof_reached: handle.get_property::<bool>("eof-reached").ok(),
            core_idle: handle.get_property::<bool>("core-idle").ok(),
            idle_active: handle.get_property::<bool>("idle-active").ok(),
            demuxer_cache_state: handle
                .get_property_string("demuxer-cache-state")
                .ok()
                .map(|s| sanitize_cache_state(&s)),
        }
    }

    /// Serializes one row. `t_s` is seconds since the run's start instant.
    pub fn to_csv_line(&self, t_s: f64) -> String {
        format!(
            "{},{:.3},{},{},{},{},{},{},{}",
            iso8601_utc(SystemTime::now()),
            t_s,
            fmt_bool(self.pause),
            fmt_f64(self.playback_time),
            fmt_bool(self.eof_reached),
            fmt_bool(self.core_idle),
            fmt_bool(self.idle_active),
            self.demuxer_cache_state.as_deref().unwrap_or(UNAVAILABLE),
            ""
        )
    }
}

fn fmt_bool(value: Option<bool>) -> String {
    match value {
        Some(true) => "true".to_string(),
        Some(false) => "false".to_string(),
        None => UNAVAILABLE.to_string(),
    }
}

fn fmt_f64(value: Option<f64>) -> String {
    match value {
        Some(v) if v.is_finite() => format!("{v:.3}"),
        _ => UNAVAILABLE.to_string(),
    }
}

/// Makes a property string safe for one CSV field and bounds its length.
///
/// `demuxer-cache-state` is a node property and can be long; a run must not be
/// able to produce an unbounded row or a row that splits into extra fields.
pub fn sanitize_cache_state(value: &str) -> String {
    let replaced = value.replace([',', '\n', '\r'], ";");
    if replaced.chars().count() <= MAX_CACHE_STATE_LEN {
        return replaced;
    }
    let mut truncated: String = replaced.chars().take(MAX_CACHE_STATE_LEN).collect();
    truncated.push_str("...");
    truncated
}

/// Owns the sampler thread.
pub struct MpvStateSampler {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    path: PathBuf,
}

impl MpvStateSampler {
    /// Spawns the sampler. `started` is the run origin used for the `t_s`
    /// column, so samples correlate with the other artifacts.
    pub fn spawn(handle: Arc<MpvHandle>, path: impl AsRef<Path>, started: Instant) -> Self {
        let path = path.as_ref().to_path_buf();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread_path = path.clone();
        let thread = std::thread::Builder::new()
            .name("muvrix-mpv-state".to_string())
            .spawn(move || run_worker(handle, &thread_path, started, &thread_stop))
            .ok();
        if thread.is_none() {
            log::error!(
                "mpv state sampler could not start; no mpv-state-samples.csv will be written"
            );
        }
        Self { stop, thread, path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Stops and joins the thread. Safe to call once; later calls are no-ops.
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for MpvStateSampler {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_worker(handle: Arc<MpvHandle>, path: &Path, started: Instant, stop: &AtomicBool) {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::warn!("mpv state sampler: cannot create {}: {e}", parent.display());
                return;
            }
        }
    }
    let file = match std::fs::File::create(path) {
        Ok(file) => file,
        Err(e) => {
            log::warn!("mpv state sampler: cannot create {}: {e}", path.display());
            return;
        }
    };
    let mut writer = std::io::BufWriter::new(file);
    if writeln!(writer, "{CANONICAL_HEADER}").is_err() {
        log::warn!("mpv state sampler: cannot write header");
        return;
    }

    log::info!(
        "mpv state sampler: {} fields/row, {} ms interval -> {}",
        CANONICAL_FIELD_COUNT,
        SAMPLE_INTERVAL.as_millis(),
        path.display()
    );

    let mut rows: u64 = 0;
    let mut last = started;
    let mut write_failed = false;

    while !stop.load(Ordering::Acquire) {
        let now = Instant::now();
        if now.saturating_duration_since(last) >= SAMPLE_INTERVAL {
            last = now;
            let sample = MpvStateSample::read(&handle);
            let line = sample.to_csv_line(now.saturating_duration_since(started).as_secs_f64());
            let ok = writeln!(writer, "{line}")
                .and_then(|_| writer.flush())
                .is_ok();
            if ok {
                rows = rows.saturating_add(1);
            } else if !write_failed {
                write_failed = true;
                log::warn!("mpv state sampler: write failed; rows will be incomplete");
            }
        }
        std::thread::sleep(STOP_POLL);
    }

    let _ = writer.flush();
    log::info!(
        "mpv state sampler stopped: rows={rows} path={}",
        path.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_has_exactly_nine_fields() {
        let fields: Vec<&str> = CANONICAL_HEADER.split(',').collect();
        assert_eq!(fields.len(), CANONICAL_FIELD_COUNT);
        assert_eq!(fields[0], "timestamp_utc");
        assert_eq!(fields[8], "notes");
    }

    #[test]
    fn unavailable_properties_are_minus_one_not_fabricated() {
        let sample = MpvStateSample::default();
        let line = sample.to_csv_line(1.0);
        let fields: Vec<&str> = line.split(',').collect();
        assert_eq!(fields.len(), CANONICAL_FIELD_COUNT, "line: {line}");
        assert_eq!(fields[2], "-1", "pause");
        assert_eq!(fields[3], "-1", "playback-time");
        assert_eq!(fields[4], "-1", "eof-reached");
        assert_eq!(fields[5], "-1", "core-idle");
        assert_eq!(fields[6], "-1", "idle-active");
        assert_eq!(fields[7], "-1", "demuxer-cache-state");
    }

    #[test]
    fn present_values_render_stably() {
        let sample = MpvStateSample {
            pause: Some(true),
            playback_time: Some(12.3456),
            eof_reached: Some(false),
            core_idle: Some(false),
            idle_active: Some(false),
            demuxer_cache_state: Some("seekable;eof=false".to_string()),
        };
        let line = sample.to_csv_line(2.5);
        assert!(line.contains(",2.500,true,12.346,false,false,false,seekable;eof=false,"));
        assert_eq!(line.split(',').count(), CANONICAL_FIELD_COUNT);
    }

    #[test]
    fn cache_state_cannot_break_csv_structure() {
        assert_eq!(sanitize_cache_state("a,b\nc\rd"), "a;b;c;d");
        let line = MpvStateSample {
            demuxer_cache_state: Some(sanitize_cache_state("a,b\nc")),
            ..MpvStateSample::default()
        }
        .to_csv_line(0.0);
        assert_eq!(line.split(',').count(), CANONICAL_FIELD_COUNT);
    }

    #[test]
    fn cache_state_is_bounded() {
        let huge = "x".repeat(10_000);
        let out = sanitize_cache_state(&huge);
        assert_eq!(out.chars().count(), MAX_CACHE_STATE_LEN + 3);
        assert!(out.ends_with("..."));
    }

    #[test]
    fn non_finite_playback_time_is_unavailable() {
        let sample = MpvStateSample {
            playback_time: Some(f64::NAN),
            ..MpvStateSample::default()
        };
        assert_eq!(sample.to_csv_line(0.0).split(',').nth(3).unwrap(), "-1");
    }

    #[test]
    fn sampling_interval_is_bounded_and_nonzero() {
        assert!(SAMPLE_INTERVAL >= Duration::from_millis(100));
        assert!(SAMPLE_INTERVAL <= Duration::from_secs(2));
    }
}
