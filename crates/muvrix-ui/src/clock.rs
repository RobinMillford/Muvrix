//! WP1 — clock sampling for the suspend discriminator.
//!
//! Three clocks are recorded per telemetry row: `CLOCK_MONOTONIC`,
//! `CLOCK_REALTIME` and (on Linux) `CLOCK_BOOTTIME`, plus the
//! `boottime - monotonic` differential. A suspend advances BOOTTIME but not
//! MONOTONIC, so a jump in the differential between two rows is the discriminant;
//! the presence of the clock does **not** by itself say anything about whether a
//! suspend happened.
//!
//! Linux-specific syscalls stay in this module. Rendering code only sees the
//! plain [`ClockSample`] struct with `Option` fields, so nothing cross-platform
//! has to know about `clock_gettime` or `CLOCK_BOOTTIME`.

/// One set of clock readings.
///
/// `None` means the clock genuinely could not be sampled on this platform or the
/// call failed; it must be rendered as "-1" in telemetry, not as `0.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockSample {
    pub monotonic_s: Option<f64>,
    pub boottime_s: Option<f64>,
    pub realtime_s: Option<f64>,
    pub boottime_minus_monotonic_s: Option<f64>,
}

impl ClockSample {
    /// Samples every available clock once.
    pub fn now() -> Self {
        let monotonic_s = sample_clock(Monotonic);
        let boottime_s = sample_clock(Boottime);
        let realtime_s = sample_clock(Realtime);
        let boottime_minus_monotonic_s = match (boottime_s, monotonic_s) {
            (Some(b), Some(m)) => Some(b - m),
            _ => None,
        };
        Self {
            monotonic_s,
            boottime_s,
            realtime_s,
            boottime_minus_monotonic_s,
        }
    }
}

/// Which clock to read. Kept as a small enum so the platform gate is in one
/// place instead of repeated at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockKind {
    Monotonic,
    Boottime,
    Realtime,
}

pub use ClockKind::{Boottime, Monotonic, Realtime};

/// Reads one clock as seconds, or `None` if this platform/call cannot provide it.
pub fn sample_clock(kind: ClockKind) -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        let id = match kind {
            Monotonic => libc::CLOCK_MONOTONIC,
            Boottime => libc::CLOCK_BOOTTIME,
            Realtime => libc::CLOCK_REALTIME,
        };
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `ts` is a valid, properly aligned `timespec` and `id` is a
        // valid clock id constant for this platform.
        let rc = unsafe { libc::clock_gettime(id, &mut ts) };
        if rc != 0 {
            return None;
        }
        Some(ts.tv_sec as f64 + ts.tv_nsec as f64 / 1_000_000_000.0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        match kind {
            Realtime => {
                use std::time::{SystemTime, UNIX_EPOCH};
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs_f64())
            }
            // No boottime/monotonic epoch on non-Linux targets here: report the
            // measurement as unavailable instead of inventing a value.
            Monotonic | Boottime => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotonic_and_realtime_are_sampled() {
        let s = ClockSample::now();
        assert!(s.monotonic_s.is_some(), "monotonic must be available");
        assert!(s.realtime_s.is_some(), "realtime must be available");
        assert!(s.monotonic_s.unwrap() > 0.0);
        assert!(s.realtime_s.unwrap() > 0.0);
    }

    #[test]
    fn boottime_differential_is_consistent() {
        let s = ClockSample::now();
        match (s.boottime_s, s.monotonic_s, s.boottime_minus_monotonic_s) {
            (Some(b), Some(m), Some(d)) => {
                assert!((b - m - d).abs() < 1e-9);
                // BOOTTIME >= MONOTONIC by construction; the difference is the
                // time spent suspended since boot.
                assert!(d >= -1e-6, "boottime - monotonic = {d}");
            }
            (None, _, None) => {
                // Platform without CLOCK_BOOTTIME: the differential must also be
                // absent, never a fabricated zero.
            }
            other => panic!("inconsistent clock sample: {other:?}"),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn boottime_is_available_on_linux() {
        // The corrected Phase 2.2 probe established CLOCK_BOOTTIME works here;
        // this test fails loudly if that ever regresses on Linux.
        assert!(sample_clock(Boottime).is_some());
    }

    #[test]
    fn clocks_advance() {
        let a = sample_clock(Monotonic).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let b = sample_clock(Monotonic).unwrap();
        assert!(b > a);
    }
}
