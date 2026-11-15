//! Phase 2.12 / causal trace transport.
//!
//! Producers enqueue fixed records into a bounded MPSC channel with
//! `try_send`; a dedicated writer drains them to CSV. The render and callback
//! threads never perform file I/O and never wait for the writer. Overflow is
//! counted and makes the trace incomplete.
//!
//! Disabled unless `MUVRIX_CAUSAL_TRACE` names an output CSV path.

use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use muvrix_player::MpvHandle;

pub const DEFAULT_CAPACITY: usize = 262_144;
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Kind {
    CallbackEntry = 1,
    CallbackWakeBridge = 2,
    CallbackEgui = 3,
    CallbackExit = 4,
    EguiRepaintRequest = 5,
    ProxySend = 6,
    UserEvent = 7,
    AboutToWaitEnter = 8,
    AboutToWaitExit = 9,
    WakeTakeDue = 10,
    ControlFlow = 11,
    RedrawRequest = 12,
    RedrawGateComplete = 13,
    RedrawRequested = 14,
    DrawEnter = 15,
    DrawExit = 16,
    UpdateEnter = 17,
    UpdateExit = 18,
    FrameSignal = 19,
    RenderDecision = 20,
    FrameInfo = 21,
    RenderEnter = 22,
    RenderExit = 23,
    TargetSchedule = 24,
    Composite = 25,
    SwapEnter = 26,
    SwapExit = 27,
    ReportSwap = 28,
    Withholding = 29,
    DropObserve = 30,
    Teardown = 31,
    PresentationClock = 32,
    PresentationPresented = 33,
    PresentationSequence = 34,
    PresentationDiscarded = 35,
    FrameCallbackRequest = 36,
    GpuTime = 37,
    FrameDeadline = 38,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CallbackEntry => "callback_entry",
            Self::CallbackWakeBridge => "callback_wake_bridge",
            Self::CallbackEgui => "callback_egui",
            Self::CallbackExit => "callback_exit",
            Self::EguiRepaintRequest => "egui_repaint_request",
            Self::ProxySend => "proxy_send",
            Self::UserEvent => "user_event",
            Self::AboutToWaitEnter => "about_to_wait_enter",
            Self::AboutToWaitExit => "about_to_wait_exit",
            Self::WakeTakeDue => "wake_take_due",
            Self::ControlFlow => "control_flow",
            Self::RedrawRequest => "redraw_request",
            Self::RedrawGateComplete => "redraw_gate_complete",
            Self::RedrawRequested => "redraw_requested",
            Self::DrawEnter => "draw_enter",
            Self::DrawExit => "draw_exit",
            Self::UpdateEnter => "update_enter",
            Self::UpdateExit => "update_exit",
            Self::FrameSignal => "frame_signal",
            Self::RenderDecision => "render_decision",
            Self::FrameInfo => "frame_info",
            Self::RenderEnter => "render_enter",
            Self::RenderExit => "render_exit",
            Self::TargetSchedule => "target_schedule",
            Self::Composite => "composite",
            Self::SwapEnter => "swap_enter",
            Self::SwapExit => "swap_exit",
            Self::ReportSwap => "report_swap",
            Self::Withholding => "withholding",
            Self::DropObserve => "drop_observe",
            Self::Teardown => "teardown",
            Self::PresentationClock => "presentation_clock",
            Self::PresentationPresented => "presentation_presented",
            Self::PresentationSequence => "presentation_sequence",
            Self::PresentationDiscarded => "presentation_discarded",
            Self::FrameCallbackRequest => "frame_callback_request",
            Self::GpuTime => "gpu_time",
            Self::FrameDeadline => "frame_deadline",
        }
    }
}

#[derive(Debug, Clone)]
struct Record {
    seq: u64,
    ts_ns: u64,
    tid: u32,
    kind: Kind,
    corr: u64,
    a: i64,
    b: i64,
    c: i64,
    d: i64,
    e: i64,
    note: &'static str,
}

struct TraceSink {
    tx: Mutex<Option<SyncSender<Record>>>,
    writer: Mutex<Option<JoinHandle<()>>>,
    path: PathBuf,
    meta: String,
    origin: Instant,
    next_seq: AtomicU64,
    dropped: AtomicU64,
    written: AtomicU64,
    in_flight: AtomicI64,
    high_water: AtomicI64,
    capacity: usize,
    enabled: AtomicBool,
}

static SINK: OnceLock<Arc<TraceSink>> = OnceLock::new();
static LAST_CALLBACK: AtomicU64 = AtomicU64::new(0);
static LAST_DRAW: AtomicU64 = AtomicU64::new(0);
static LAST_DROP_TOTAL: AtomicI64 = AtomicI64::new(-1);
static LAST_DROP_DECODER: AtomicI64 = AtomicI64::new(-1);
static DROP_STOP: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static DROP_THREAD: OnceLock<Mutex<Option<JoinHandle<()>>>> = OnceLock::new();

fn tid() -> u32 {
    #[cfg(target_os = "linux")]
    {
        unsafe { libc::syscall(libc::SYS_gettid) as u32 }
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn metadata() -> String {
    format!(
        "schema_version={}\nrun_id={}\nbinary_sha256={}\ngit_branch={}\ngit_head={}\nlibmpv={}\nclient_api={}\nmonitor={}\nsource={}\nvideo_sync={}\ndisplay_fps_override={}\negl_swap_interval={}\n",
        SCHEMA_VERSION,
        env_or("MUVRIX_CAUSAL_TRACE_RUN_ID", "unknown"),
        env_or("MUVRIX_CAUSAL_TRACE_BINARY_SHA256", "unknown"),
        env_or("MUVRIX_CAUSAL_TRACE_GIT_BRANCH", "unknown"),
        env_or("MUVRIX_CAUSAL_TRACE_GIT_HEAD", "unknown"),
        env_or("MUVRIX_CAUSAL_TRACE_LIBMPV", "unknown"),
        env_or("MUVRIX_CAUSAL_TRACE_CLIENT_API", "unknown"),
        env_or("MUVRIX_CAUSAL_TRACE_MONITOR", "unknown"),
        env_or("MUVRIX_CAUSAL_TRACE_SOURCE", "unknown"),
        env_or("MUVRIX_VIDEO_SYNC", "audio"),
        env_or("MUVRIX_DISPLAY_FPS_OVERRIDE", "none"),
        env_or("MUVRIX_EGL_SWAP_INTERVAL", "0"),
    )
}

impl TraceSink {
    fn start(path: PathBuf, capacity: usize) -> Result<Arc<Self>, String> {
        let (tx, rx) = sync_channel::<Record>(capacity);
        let meta = metadata();
        let sink = Arc::new(Self {
            tx: Mutex::new(Some(tx)),
            writer: Mutex::new(None),
            path: path.clone(),
            meta,
            origin: Instant::now(),
            next_seq: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            written: AtomicU64::new(0),
            in_flight: AtomicI64::new(0),
            high_water: AtomicI64::new(0),
            capacity,
            enabled: AtomicBool::new(true),
        });
        let writer_sink = Arc::clone(&sink);
        let writer = std::thread::Builder::new()
            .name("muvrix-causal-trace".into())
            .spawn(move || write_worker(writer_sink, rx))
            .map_err(|e| format!("spawn causal trace writer: {e}"))?;
        *sink.writer.lock().unwrap() = Some(writer);
        Ok(sink)
    }

    #[allow(clippy::too_many_arguments)] // fixed-width record fields
    fn record(
        &self,
        kind: Kind,
        corr: u64,
        a: i64,
        b: i64,
        c: i64,
        d: i64,
        e: i64,
        note: &'static str,
    ) -> u64 {
        if !self.enabled.load(Ordering::Relaxed) {
            return 0;
        }
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let rec = Record {
            seq,
            ts_ns: self.origin.elapsed().as_nanos() as u64,
            tid: tid(),
            kind,
            corr,
            a,
            b,
            c,
            d,
            e,
            note,
        };
        let in_flight = self.in_flight.fetch_add(1, Ordering::Relaxed) + 1;
        self.high_water.fetch_max(in_flight, Ordering::Relaxed);
        let guard = match self.tx.lock() {
            Ok(g) => g,
            Err(_) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                self.in_flight.fetch_sub(1, Ordering::Relaxed);
                return seq;
            }
        };
        let Some(tx) = guard.as_ref() else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            self.in_flight.fetch_sub(1, Ordering::Relaxed);
            return seq;
        };
        match tx.try_send(rec) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                self.in_flight.fetch_sub(1, Ordering::Relaxed);
            }
        }
        seq
    }

    fn shutdown(&self) {
        self.enabled.store(false, Ordering::Release);
        let tx = self.tx.lock().ok().and_then(|mut g| g.take());
        drop(tx);
        if let Some(writer) = self.writer.lock().ok().and_then(|mut g| g.take()) {
            let _ = writer.join();
        }
    }
}

fn write_worker(sink: Arc<TraceSink>, rx: Receiver<Record>) {
    let file = match std::fs::File::create(&sink.path) {
        Ok(f) => f,
        Err(e) => {
            log::error!("causal trace: cannot create {}: {e}", sink.path.display());
            return;
        }
    };
    let mut out = BufWriter::new(file);
    if writeln!(out, "# muvrix causal trace schema={}", SCHEMA_VERSION).is_err() {
        return;
    }
    for line in sink.meta.lines() {
        let _ = writeln!(out, "# {line}");
    }
    let _ = writeln!(
        out,
        "# queue_capacity={} header=seq,ts_ns,tid,kind,corr,a,b,c,d,e,note",
        sink.capacity
    );
    let mut written: u64 = 0;
    let mut flush_count: u64 = 0;
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(rec) => {
                sink.in_flight.fetch_sub(1, Ordering::Relaxed);
                if writeln!(
                    out,
                    "{},{},{},{},{},{},{},{},{},{},{}",
                    rec.seq,
                    rec.ts_ns,
                    rec.tid,
                    rec.kind.as_str(),
                    rec.corr,
                    rec.a,
                    rec.b,
                    rec.c,
                    rec.d,
                    rec.e,
                    rec.note
                )
                .is_err()
                {
                    log::warn!("causal trace: write failed");
                    break;
                }
                written += 1;
                sink.written.store(written, Ordering::Relaxed);
                if written.is_multiple_of(4096) {
                    let _ = out.flush();
                    flush_count += 1;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if out.flush().is_ok() {
                    flush_count += 1;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let dropped = sink.dropped.load(Ordering::Relaxed);
    let high_water = sink.high_water.load(Ordering::Relaxed);
    let complete = dropped == 0;
    let _ = out.flush();
    let _ = writeln!(
        out,
        "# footer status={} written={} dropped={} high_water={} capacity={} flushes={}",
        if complete { "complete" } else { "invalid" },
        written,
        dropped,
        high_water,
        sink.capacity,
        flush_count
    );
    let _ = out.flush();
    log::info!(
        "causal trace: status={} written={} dropped={} high_water={} capacity={} flushes={} path={}",
        if complete { "complete" } else { "invalid" },
        written,
        dropped,
        high_water,
        sink.capacity,
        flush_count,
        sink.path.display()
    );
}

/// Starts the trace from environment variables. Idempotent; no-op when the
/// variable is absent or empty.
pub fn init_from_env() {
    if SINK.get().is_some() {
        return;
    }
    let Some(path) = std::env::var("MUVRIX_CAUSAL_TRACE")
        .ok()
        .filter(|p| !p.trim().is_empty())
    else {
        return;
    };
    let capacity = std::env::var("MUVRIX_CAUSAL_TRACE_CAPACITY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v >= 1024)
        .unwrap_or(DEFAULT_CAPACITY);
    match TraceSink::start(PathBuf::from(path), capacity) {
        Ok(sink) => {
            let _ = SINK.set(sink);
            log::info!("causal trace enabled (capacity={capacity})");
        }
        Err(e) => log::error!("causal trace disabled: {e}"),
    }
}

fn sink() -> Option<&'static TraceSink> {
    SINK.get().map(|s| s.as_ref())
}

/// Records one event. Returns the event sequence (0 when disabled).
pub fn record(kind: Kind, corr: u64, a: i64, b: i64, c: i64, note: &'static str) -> u64 {
    sink()
        .map(|s| s.record(kind, corr, a, b, c, 0, 0, note))
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)] // fixed-width record fields
pub fn record5(
    kind: Kind,
    corr: u64,
    a: i64,
    b: i64,
    c: i64,
    d: i64,
    e: i64,
    note: &'static str,
) -> u64 {
    sink()
        .map(|s| s.record(kind, corr, a, b, c, d, e, note))
        .unwrap_or(0)
}

pub fn set_last_callback(seq: u64) {
    LAST_CALLBACK.store(seq, Ordering::Relaxed);
}

pub fn last_callback() -> u64 {
    LAST_CALLBACK.load(Ordering::Relaxed)
}

pub fn set_last_draw(seq: u64) {
    LAST_DRAW.store(seq, Ordering::Relaxed);
}

pub fn last_draw() -> u64 {
    LAST_DRAW.load(Ordering::Relaxed)
}

/// Latest logical `frame-drop-count` / decoder drop count observed by the
/// asynchronous drop watcher. The runner uses this lock-free snapshot when it
/// records a frame deadline; it never performs a synchronous mpv property read
/// from the GL thread for per-frame telemetry.
pub fn drop_snapshot() -> (i64, i64) {
    (
        LAST_DROP_TOTAL.load(Ordering::Relaxed),
        LAST_DROP_DECODER.load(Ordering::Relaxed),
    )
}

/// Raw `CLOCK_MONOTONIC` nanoseconds, used to correlate the compositor's
/// `wp_presentation` timestamp with the EGL swap timestamp. The clock id is
/// recorded separately; no conversion is assumed.
pub fn monotonic_ns() -> i64 {
    #[cfg(target_os = "linux")]
    {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
        ts.tv_sec
            .saturating_mul(1_000_000_000)
            .saturating_add(ts.tv_nsec)
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// Starts a fast-but-approximate drop observation thread. Its timestamps are
/// observation times, not the exact instant MPV discarded a frame.
pub fn install_drop_watcher(handle: Arc<MpvHandle>) {
    if sink().is_none() || DROP_STOP.get().is_some() {
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    if DROP_STOP.set(stop.clone()).is_err() {
        return;
    }
    let thread = std::thread::Builder::new()
        .name("muvrix-causal-drop".into())
        .spawn(move || {
            let mut prev: Option<(i64, i64, i64, i64, i64)> = None;
            while !stop.load(Ordering::Acquire) {
                let drop = handle.get_property::<i64>("frame-drop-count").unwrap_or(-1);
                let dec = handle
                    .get_property::<i64>("decoder-frame-drop-count")
                    .unwrap_or(-1);
                let mist = handle
                    .get_property::<i64>("mistimed-frame-count")
                    .unwrap_or(-1);
                let delayed = handle
                    .get_property::<i64>("vo-delayed-frame-count")
                    .unwrap_or(-1);
                let dsync = handle
                    .get_property::<bool>("display-sync-active")
                    .map(|v| v as i64)
                    .unwrap_or(-1);
                LAST_DROP_TOTAL.store(drop, Ordering::Relaxed);
                LAST_DROP_DECODER.store(dec, Ordering::Relaxed);
                let now = (drop, dec, mist, delayed, dsync);
                if prev != Some(now) {
                    record5(
                        Kind::DropObserve,
                        last_draw(),
                        drop,
                        dec,
                        mist,
                        delayed,
                        dsync,
                        "sampled_20ms",
                    );
                    prev = Some(now);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        })
        .ok();
    if let Some(thread) = thread {
        *DROP_THREAD.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(thread);
    }
}

pub fn shutdown() {
    if let Some(stop) = DROP_STOP.get() {
        stop.store(true, Ordering::Release);
    }
    if let Some(thread) = DROP_THREAD
        .get()
        .and_then(|m| m.lock().ok().and_then(|mut g| g.take()))
    {
        let _ = thread.join();
    }
    if let Some(s) = sink() {
        s.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_encoding_keeps_kind_and_values() {
        let r = Record {
            seq: 7,
            ts_ns: 42,
            tid: 3,
            kind: Kind::FrameInfo,
            corr: 11,
            a: 1,
            b: 2,
            c: 3,
            d: 4,
            e: 5,
            note: "test",
        };
        assert_eq!(r.kind.as_str(), "frame_info");
        assert_eq!(r.corr, 11);
        assert_eq!(r.a + r.b + r.c + r.d + r.e, 15);
    }

    #[test]
    fn schema_version_is_stable() {
        assert_eq!(SCHEMA_VERSION, 1);
    }

    #[test]
    fn concurrent_producers_keep_unique_sequences_and_complete_footer() {
        let path = std::env::temp_dir().join(format!(
            "muvrix-causal-trace-test-{}.csv",
            std::process::id()
        ));
        let sink = TraceSink::start(path.clone(), 4096).expect("start trace");
        let mut threads = Vec::new();
        for producer in 0..4u64 {
            let sink = Arc::clone(&sink);
            threads.push(std::thread::spawn(move || {
                for i in 0..1000u64 {
                    sink.record(Kind::FrameInfo, producer, i as i64, 0, 0, 0, 0, "test");
                }
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        sink.shutdown();
        let text = std::fs::read_to_string(&path).expect("trace file");
        let events: Vec<&str> = text
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .collect();
        assert_eq!(events.len(), 4000);
        let mut seqs = std::collections::HashSet::new();
        for line in &events {
            let seq: u64 = line.split(',').next().unwrap().parse().unwrap();
            assert!(seqs.insert(seq), "duplicate sequence {seq}");
        }
        assert!(text.contains("footer status=complete"), "{text}");
        let _ = std::fs::remove_file(path);
    }
}
