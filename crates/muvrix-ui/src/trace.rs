//! Phase 2.5 — bounded, low-overhead event trace.
//!
//! The per-second telemetry row answers "did the pipeline progress", but not
//! "which stage stopped first". This module records the event-loop and render
//! chain with a monotonically increasing sequence number so a frozen run can be
//! reconstructed after the fact.
//!
//! Design rules (Phase 2.5 brief, Part 3):
//!
//! - the render thread never blocks: `record` only uses `try_lock`, and a
//!   contended record is counted as dropped instead of waited for;
//! - no allocation on the hot path: the ring is pre-allocated and the detail is
//!   a `&'static str` plus two integers;
//! - no file I/O from the render thread: rows are written only by `dump`, which
//!   runs on the telemetry thread (stall/recovery) or after rendering stopped
//!   (teardown);
//! - no mpv or GL call is made from here.
//!
//! Disabled unless `MUVRIX_EVENT_TRACE` names an output file, so ordinary runs
//! pay one relaxed atomic read per event.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Ring capacity. 8192 events is a few seconds of a healthy run and hours of a
/// stalled one, which is exactly where the interesting data is.
///
/// The buffer is a sequence-indexed circular ring: slot `seq % CAPACITY` is
/// overwritten once the ring is full, so it always retains the *last* `CAPACITY`
/// events and loses the oldest ones. A healthy run emits roughly 1200 events per
/// second (a 60 Hz paint chain), so ~6.8 s fits; the ten-second WP1 preflight
/// emitted 12063 events and lost the t=1–3 s window that contained the stall it
/// was meant to explain. A five-second diagnostic fits without growing the ring.
pub const CAPACITY: usize = 8192;

/// Event kinds on the wake/render chain, in pipeline order.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Ev {
    // --- wake path (mpv side -> winit)
    UpdateCallback = 0,
    ProxySend = 1,
    UserEvent = 2,
    DeadlineSet = 3,
    DeadlineReplaced = 4,
    DeadlineRetained = 5,
    DeadlineDue = 6,
    WakeDelivered = 7,
    AboutToWait = 8,
    ControlFlow = 9,
    // --- paint path
    RedrawRequested = 10,
    DrawEnter = 11,
    DrawExit = 12,
    RenderUpdate = 13,
    RenderEnter = 14,
    RenderExit = 15,
    SwapEnter = 16,
    SwapExit = 17,
    ReportSwap = 18,
    // --- supervision
    Nudge = 19,
    StallStart = 20,
    StallRecovery = 21,
    RunDeadline = 22,
    SupervisorKill = 23,
    Teardown = 24,
}

impl Ev {
    pub fn as_str(self) -> &'static str {
        match self {
            Ev::UpdateCallback => "update_callback",
            Ev::ProxySend => "proxy_send",
            Ev::UserEvent => "user_event",
            Ev::DeadlineSet => "deadline_set",
            Ev::DeadlineReplaced => "deadline_replaced",
            Ev::DeadlineRetained => "deadline_retained",
            Ev::DeadlineDue => "deadline_due",
            Ev::WakeDelivered => "wake_delivered",
            Ev::AboutToWait => "about_to_wait",
            Ev::ControlFlow => "control_flow",
            Ev::RedrawRequested => "redraw_requested",
            Ev::DrawEnter => "draw_enter",
            Ev::DrawExit => "draw_exit",
            Ev::RenderUpdate => "render_update",
            Ev::RenderEnter => "render_enter",
            Ev::RenderExit => "render_exit",
            Ev::SwapEnter => "swap_enter",
            Ev::SwapExit => "swap_exit",
            Ev::ReportSwap => "report_swap",
            Ev::Nudge => "nudge",
            Ev::StallStart => "stall_start",
            Ev::StallRecovery => "stall_recovery",
            Ev::RunDeadline => "run_deadline",
            Ev::SupervisorKill => "supervisor_kill",
            Ev::Teardown => "teardown",
        }
    }
}

#[derive(Clone, Copy)]
struct Entry {
    seq: u64,
    t_ns: u64,
    tid: u32,
    ev: Ev,
    reason: &'static str,
    a: i64,
    b: i64,
}

struct Trace {
    ring: Mutex<Vec<Entry>>,
    next_seq: AtomicU64,
    dropped: AtomicU64,
    enabled: AtomicBool,
    path: &'static str,
    origin: Instant,
}

static TRACE: OnceLock<Trace> = OnceLock::new();

fn trace() -> &'static Trace {
    TRACE.get_or_init(|| {
        let path = match std::env::var("MUVRIX_EVENT_TRACE") {
            Ok(p) if !p.trim().is_empty() => Box::leak(p.into_boxed_str()),
            _ => "",
        };
        Trace {
            ring: Mutex::new(Vec::with_capacity(CAPACITY)),
            next_seq: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            enabled: AtomicBool::new(!path.is_empty()),
            path,
            origin: Instant::now(),
        }
    })
}

pub fn enabled() -> bool {
    trace().enabled.load(Ordering::Relaxed)
}

fn tid() -> u32 {
    #[cfg(target_os = "linux")]
    {
        // `gettid` is the kernel thread id the supervisor's /proc entries use,
        // so trace rows can be matched against `wchan`/`stack` dumps.
        unsafe { libc::syscall(libc::SYS_gettid) as u32 }
    }
    #[cfg(not(target_os = "linux"))]
    {
        // No portable numeric thread id; the trace still carries ordering.
        0
    }
}

/// Records one event. Never blocks, never allocates, never logs.
pub fn record(ev: Ev, reason: &'static str, a: i64, b: i64) {
    let t = trace();
    if !t.enabled.load(Ordering::Relaxed) {
        return;
    }
    let seq = t.next_seq.fetch_add(1, Ordering::Relaxed);
    let entry = Entry {
        seq,
        t_ns: t.origin.elapsed().as_nanos() as u64,
        tid: tid(),
        ev,
        reason,
        a,
        b,
    };
    match t.ring.try_lock() {
        Ok(mut ring) => {
            if ring.len() == CAPACITY {
                // Ring semantics: overwrite the oldest slot. `seq` still orders
                // the events, so a wrapped buffer is readable.
                let slot = seq as usize % CAPACITY;
                ring[slot] = entry;
            } else {
                ring.push(entry);
            }
        }
        Err(_) => {
            // Contended: drop rather than block the render thread.
            t.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// First line of a dump. Names the phase that owns this module's schema.
///
/// Kept as a function so the label is asserted by a test: the header is the only
/// thing that tells a reader which phase's event vocabulary a trace file uses,
/// and a stale label mislabelled every Phase 2.6 artifact as Phase 2.5.
fn header(reason: &str, events: usize, dropped: u64) -> String {
    format!(
        "# phase-2.6 event trace: reason={reason} events={events} dropped={dropped} capacity={CAPACITY}\n"
    )
}

/// Monotonic sequence of the last recorded event (diagnostic).
pub fn last_seq() -> u64 {
    trace().next_seq.load(Ordering::Relaxed)
}

/// Writes the buffer to `MUVRIX_EVENT_TRACE`, oldest first, with `reason`.
///
/// Called from the telemetry thread on a stall transition and from the main
/// thread during teardown (after rendering has stopped). Best effort: a failure
/// is logged once and never propagates.
pub fn dump(reason: &str) {
    let t = trace();
    if !t.enabled.load(Ordering::Relaxed) || t.path.is_empty() {
        return;
    }
    let Ok(ring) = t.ring.lock() else { return };
    let dropped = t.dropped.load(Ordering::Relaxed);
    let mut events: Vec<Entry> = ring.clone();
    drop(ring);
    events.sort_by_key(|e| e.seq);

    let mut out = String::with_capacity(events.len() * 48 + 256);
    out.push_str(&header(reason, events.len(), dropped));
    out.push_str("# seq t_ms tid event reason a b\n");
    for e in &events {
        out.push_str(&format!(
            "{} {:.3} {} {} {} {} {}\n",
            e.seq,
            e.t_ns as f64 / 1e6,
            e.tid,
            e.ev.as_str(),
            e.reason,
            e.a,
            e.b
        ));
    }
    match std::fs::write(t.path, out) {
        Ok(()) => log::info!(
            "event trace written to {} ({} events, {dropped} dropped, reason={reason})",
            t.path,
            events.len()
        ),
        Err(e) => log::warn!("cannot write event trace to {}: {e}", t.path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_trace_records_nothing_and_is_cheap() {
        // Without MUVRIX_EVENT_TRACE no event is retained; this test only proves
        // the calls are safe with the feature off (the process-wide OnceLock is
        // initialised once, so the flag cannot be flipped mid-suite).
        if !enabled() {
            let before = last_seq();
            record(Ev::UserEvent, "test", 0, 0);
            assert_eq!(last_seq(), before, "disabled trace must not allocate seqs");
        }
    }

    #[test]
    fn dump_header_names_the_phase_that_owns_the_schema() {
        // The header is the only marker of which event vocabulary a trace file
        // uses. It said "phase-2.5" on Phase 2.6 artifacts.
        let h = header("teardown", 3, 1);
        assert!(h.starts_with("# phase-2.6 event trace:"), "header: {h}");
        assert!(!h.contains("phase-2.5"), "stale phase label: {h}");
        assert!(h.contains("reason=teardown"), "header: {h}");
        assert!(h.contains("events=3"), "header: {h}");
        assert!(h.contains("dropped=1"), "header: {h}");
        assert!(h.contains(&format!("capacity={CAPACITY}")), "header: {h}");
    }

    #[test]
    fn event_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for ev in [
            Ev::UpdateCallback,
            Ev::ProxySend,
            Ev::UserEvent,
            Ev::DeadlineSet,
            Ev::DeadlineReplaced,
            Ev::DeadlineRetained,
            Ev::DeadlineDue,
            Ev::WakeDelivered,
            Ev::AboutToWait,
            Ev::ControlFlow,
            Ev::RedrawRequested,
            Ev::DrawEnter,
            Ev::DrawExit,
            Ev::RenderUpdate,
            Ev::RenderEnter,
            Ev::RenderExit,
            Ev::SwapEnter,
            Ev::SwapExit,
            Ev::ReportSwap,
            Ev::Nudge,
            Ev::StallStart,
            Ev::StallRecovery,
            Ev::RunDeadline,
            Ev::SupervisorKill,
            Ev::Teardown,
        ] {
            assert!(seen.insert(ev.as_str()), "duplicate name {}", ev.as_str());
        }
    }
}
