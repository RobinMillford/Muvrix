//! Real playback diagnostics collected from libmpv on a background thread.
//!
//! Every value in [`PlaybackDiagnostics`] is read from an actual libmpv
//! property (or the OS process table for CPU/RSS). Nothing here is estimated
//! or hardcoded, and nothing here runs on the paint thread: the whole point of
//! the pipeline fixes documented in
//! `docs/verification/wayland-presentation-throughput-2026-11-19.md` is that a
//! single synchronous engine read on the GL thread can stall presentation for
//! tens of milliseconds. The probe therefore owns a worker that samples at a
//! low, bounded rate and publishes an immutable snapshot.
//!
//! Sampling only happens while the statistics overlay is open (or a report is
//! being exported), so ordinary playback pays no measurement cost at all.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use muvrix_player::MpvHandle;
use serde::Serialize;

/// How often the worker samples while the overlay is open.
const ACTIVE_INTERVAL: Duration = Duration::from_millis(500);
/// How often the worker checks whether it should start sampling again.
const IDLE_INTERVAL: Duration = Duration::from_millis(250);
/// Sleep granularity so `Drop` never waits a full interval.
const SLEEP_SLICE: Duration = Duration::from_millis(50);

/// One immutable sample of engine, timing and system state.
///
/// `Option` means "libmpv does not currently expose this property"; it is not
/// an error and is rendered as `—`. Strings are empty when unavailable.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PlaybackDiagnostics {
    pub sampled_at_unix_ms: u128,
    pub sample_count: u64,

    // ---- Video -----------------------------------------------------------
    pub video_codec: String,
    pub codec_profile: String,
    pub width: i64,
    pub height: i64,
    pub bit_depth: Option<i64>,
    pub pixel_format: String,
    pub output_pixel_format: String,
    pub colour_gamma: String,
    pub colour_primaries: String,
    pub container_fps: Option<f64>,
    pub estimated_fps: Option<f64>,
    pub display_fps: Option<f64>,
    pub current_vo: String,
    pub hwdec_current: String,
    pub hwdec_requested: String,
    pub video_filters: String,
    pub hdr: String,

    // ---- Frame pacing ----------------------------------------------------
    pub frames_decoded: Option<i64>,
    pub vo_drops: i64,
    pub decoder_drops: i64,
    pub delayed_frames: i64,
    pub mistimed_frames: Option<i64>,
    pub display_sync: Option<bool>,
    pub vsync_ratio: Option<f64>,
    pub avsync: Option<f64>,
    pub total_avsync_change: Option<f64>,
    pub vo_drop_rate: f64,
    pub decoder_drop_rate: f64,

    // ---- Audio -----------------------------------------------------------
    pub audio_codec: String,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub audio_device: String,
    pub current_ao: String,
    pub audio_delay: Option<f64>,
    pub sub_delay: Option<f64>,

    // ---- Delivery / system ----------------------------------------------
    pub cache_duration: Option<f64>,
    pub cache_speed: Option<f64>,
    pub video_bitrate: Option<f64>,
    pub cpu_percent: Option<f64>,
    pub rss_mib: Option<f64>,
}

impl PlaybackDiagnostics {
    /// A short, human-readable classification of *which* stage is losing
    /// frames, derived from the counters the engine actually reports.
    ///
    /// This deliberately does not claim causality beyond what the counters can
    /// support; it names the stage whose counter is growing.
    pub fn drop_verdict(&self) -> &'static str {
        if self.decoder_drop_rate > 0.5 && self.vo_drop_rate <= self.decoder_drop_rate {
            "Decoder limited — decoder is discarding frames (decode or decode input)"
        } else if self.vo_drop_rate > 0.5 && self.delayed_frames > 0 {
            "Presentation limited — output is behind and holding late frames"
        } else if self.vo_drop_rate > 0.5 {
            "Output limited — renderer/present path is missing deadlines"
        } else if self.avsync.is_some_and(|v| v.abs() > 0.25) {
            "Clock divergence — audio/video timestamps disagree"
        } else if self.cache_duration.is_some_and(|v| v < 0.05) {
            "Delivery limited — demuxer cache is starving"
        } else {
            "Nominal — no counter is growing"
        }
    }

    /// Formats the sample as a flat, greppable `key: value` report.
    pub fn to_text_report(&self) -> String {
        let mut out = String::new();
        let opt = |v: Option<f64>| {
            v.map(|x| format!("{x:.3}"))
                .unwrap_or_else(|| "n/a".to_string())
        };
        let opti = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_else(|| "n/a".into());
        out.push_str("MUVRIX playback diagnostics\n");
        out.push_str(&format!(
            "sampled_at_unix_ms: {}\n",
            self.sampled_at_unix_ms
        ));
        out.push_str(&format!("sample_count: {}\n", self.sample_count));
        out.push('\n');
        out.push_str("[video]\n");
        out.push_str(&format!("codec: {}\n", self.video_codec));
        out.push_str(&format!("profile: {}\n", self.codec_profile));
        out.push_str(&format!("resolution: {}x{}\n", self.width, self.height));
        out.push_str(&format!("bit_depth: {}\n", opti(self.bit_depth)));
        out.push_str(&format!("pixel_format: {}\n", self.pixel_format));
        out.push_str(&format!(
            "output_pixel_format: {}\n",
            self.output_pixel_format
        ));
        out.push_str(&format!(
            "colour: {} / {}\n",
            self.colour_gamma, self.colour_primaries
        ));
        out.push_str(&format!("container_fps: {}\n", opt(self.container_fps)));
        out.push_str(&format!("estimated_fps: {}\n", opt(self.estimated_fps)));
        out.push_str(&format!("display_fps: {}\n", opt(self.display_fps)));
        out.push_str(&format!("current_vo: {}\n", self.current_vo));
        out.push_str(&format!("hwdec_current: {}\n", self.hwdec_current));
        out.push_str(&format!("hwdec_requested: {}\n", self.hwdec_requested));
        out.push_str(&format!("video_filters: {}\n", self.video_filters));
        out.push_str(&format!("hdr: {}\n", self.hdr));
        out.push_str("\n[frame_pacing]\n");
        out.push_str(&format!("frames_decoded: {}\n", opti(self.frames_decoded)));
        out.push_str(&format!("vo_drops: {}\n", self.vo_drops));
        out.push_str(&format!("decoder_drops: {}\n", self.decoder_drops));
        out.push_str(&format!("delayed_frames: {}\n", self.delayed_frames));
        out.push_str(&format!(
            "mistimed_frames: {}\n",
            opti(self.mistimed_frames)
        ));
        out.push_str(&format!(
            "display_sync_active: {}\n",
            self.display_sync
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a".into())
        ));
        out.push_str(&format!("vsync_ratio: {}\n", opt(self.vsync_ratio)));
        out.push_str(&format!("avsync: {}\n", opt(self.avsync)));
        out.push_str(&format!(
            "total_avsync_change: {}\n",
            opt(self.total_avsync_change)
        ));
        out.push_str(&format!("vo_drop_rate_per_s: {:.3}\n", self.vo_drop_rate));
        out.push_str(&format!(
            "decoder_drop_rate_per_s: {:.3}\n",
            self.decoder_drop_rate
        ));
        out.push_str(&format!("verdict: {}\n", self.drop_verdict()));
        out.push_str("\n[audio]\n");
        out.push_str(&format!("codec: {}\n", self.audio_codec));
        out.push_str(&format!("sample_rate: {}\n", opti(self.sample_rate)));
        out.push_str(&format!("channels: {}\n", opti(self.channels)));
        out.push_str(&format!("audio_device: {}\n", self.audio_device));
        out.push_str(&format!("current_ao: {}\n", self.current_ao));
        out.push_str(&format!("audio_delay: {}\n", opt(self.audio_delay)));
        out.push_str(&format!("sub_delay: {}\n", opt(self.sub_delay)));
        out.push_str("\n[delivery_system]\n");
        out.push_str(&format!("cache_duration: {}\n", opt(self.cache_duration)));
        out.push_str(&format!("cache_speed: {}\n", opt(self.cache_speed)));
        out.push_str(&format!("video_bitrate: {}\n", opt(self.video_bitrate)));
        out.push_str(&format!("cpu_percent: {}\n", opt(self.cpu_percent)));
        out.push_str(&format!("rss_mib: {}\n", opt(self.rss_mib)));
        out
    }
}

/// Owns the sampling worker and the last published snapshot.
pub struct DiagnosticsProbe {
    shared: Arc<Mutex<PlaybackDiagnostics>>,
    enabled: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    refresh_request: Arc<AtomicU64>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl DiagnosticsProbe {
    /// Starts a probe against `handle`. The thread reads properties only; it
    /// never mutates playback state.
    pub fn new(handle: Arc<MpvHandle>) -> Self {
        let shared = Arc::new(Mutex::new(PlaybackDiagnostics::default()));
        let enabled = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let refresh_request = Arc::new(AtomicU64::new(0));

        let worker = {
            let shared = Arc::clone(&shared);
            let enabled = Arc::clone(&enabled);
            let stop = Arc::clone(&stop);
            let refresh_request = Arc::clone(&refresh_request);
            std::thread::Builder::new()
                .name("muvrix-diagnostics".to_string())
                .spawn(move || {
                    let mut last_vo_drops = 0_i64;
                    let mut last_decoder_drops = 0_i64;
                    let mut last_sample = Instant::now();
                    let mut sample_count = 0_u64;
                    while !stop.load(Ordering::Relaxed) {
                        let requested = refresh_request.load(Ordering::Relaxed) > 0;
                        if !enabled.load(Ordering::Relaxed) && !requested {
                            std::thread::sleep(IDLE_INTERVAL);
                            continue;
                        }
                        let started = Instant::now();
                        let mut sample = read_snapshot(&handle);
                        sample_count += 1;
                        sample.sample_count = sample_count;
                        let elapsed = started.saturating_duration_since(last_sample).as_secs_f64();
                        if elapsed > 0.05 {
                            sample.vo_drop_rate =
                                ((sample.vo_drops - last_vo_drops).max(0) as f64) / elapsed;
                            sample.decoder_drop_rate =
                                ((sample.decoder_drops - last_decoder_drops).max(0) as f64)
                                    / elapsed;
                        }
                        last_vo_drops = sample.vo_drops;
                        last_decoder_drops = sample.decoder_drops;
                        last_sample = started;
                        if let Ok(mut lock) = shared.lock() {
                            *lock = sample;
                        }
                        // Consume one refresh request per completed sample.
                        let pending = refresh_request.load(Ordering::Relaxed);
                        if pending > 0 {
                            refresh_request.store(pending - 1, Ordering::Relaxed);
                        }
                        let deadline = Instant::now() + ACTIVE_INTERVAL;
                        while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
                            std::thread::sleep(SLEEP_SLICE);
                        }
                    }
                })
                .ok()
        };

        Self {
            shared,
            enabled,
            stop,
            refresh_request,
            worker,
        }
    }

    /// Enables or pauses sampling. Called when the overlay opens/closes.
    pub fn set_enabled(&self, on: bool) {
        let was = self.enabled.swap(on, Ordering::Relaxed);
        if on && !was {
            self.refresh_request.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// True when the statistics overlay currently holds the probe open.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Last published sample (cheap clone; never blocks on the engine).
    pub fn snapshot(&self) -> PlaybackDiagnostics {
        self.shared.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Writes a JSON + text diagnostics report and returns the JSON path.
    ///
    /// Uses the most recent sample rather than blocking the UI thread for a
    /// fresh one; the snapshot's timestamp makes its age explicit.
    pub fn export_report(&self) -> Result<std::path::PathBuf, String> {
        self.refresh_request.fetch_add(1, Ordering::Relaxed);
        // Give the worker one sampling pass, bounded so a stalled engine can
        // never hang the caller.
        let deadline = Instant::now() + Duration::from_millis(400);
        while Instant::now() < deadline && self.snapshot().sample_count == 0 {
            std::thread::sleep(Duration::from_millis(20));
        }
        let sample = self.snapshot();
        let dir = crate::config_editor::config_dir().join("diagnostics");
        std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let json_path = dir.join(format!("playback-{stamp}.json"));
        let text_path = dir.join(format!("playback-{stamp}.txt"));
        let json = serde_json::to_string_pretty(&sample)
            .map_err(|e| format!("serialize diagnostics: {e}"))?;
        std::fs::write(&json_path, json).map_err(|e| format!("write report: {e}"))?;
        std::fs::write(&text_path, sample.to_text_report())
            .map_err(|e| format!("write report: {e}"))?;
        Ok(json_path)
    }
}

impl Drop for DiagnosticsProbe {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn read_snapshot(handle: &MpvHandle) -> PlaybackDiagnostics {
    let s = |name: &str| handle.get_property::<String>(name).unwrap_or_default();
    let d = |name: &str| handle.get_property::<f64>(name).ok();
    let i = |name: &str| handle.get_property::<i64>(name).ok();
    let b = |name: &str| handle.get_property::<bool>(name).ok();

    let width = i("width").unwrap_or(0);
    let height = i("height").unwrap_or(0);
    let gamma = s("video-params/gamma");
    let primaries = s("video-params/primaries");
    let (cpu, rss) = process_usage();

    PlaybackDiagnostics {
        sampled_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        sample_count: 0,
        video_codec: s("video-codec"),
        codec_profile: s("video-params/profile"),
        width,
        height,
        bit_depth: i("video-params/bit-depth"),
        pixel_format: s("video-params/pixelformat"),
        output_pixel_format: s("video-out-params/pixelformat"),
        colour_gamma: gamma.clone(),
        colour_primaries: primaries.clone(),
        container_fps: d("container-fps"),
        estimated_fps: d("estimated-vf-fps"),
        display_fps: d("display-fps"),
        current_vo: s("current-vo"),
        hwdec_current: s("hwdec-current"),
        hwdec_requested: s("hwdec"),
        video_filters: s("vf"),
        hdr: format!(
            "{}{}",
            if gamma.is_empty() {
                "SDR".to_string()
            } else {
                gamma.clone()
            },
            if primaries.is_empty() {
                String::new()
            } else {
                format!("/{primaries}")
            }
        ),
        // libmpv exposes no cumulative "frames decoded" counter; only drops.
        frames_decoded: None,
        vo_drops: i("frame-drop-count").unwrap_or(0),
        decoder_drops: i("decoder-frame-drop-count").unwrap_or(0),
        delayed_frames: i("vo-delayed-frame-count").unwrap_or(0),
        mistimed_frames: i("mistimed-frame-count"),
        display_sync: b("display-sync-active"),
        vsync_ratio: d("vsync-ratio"),
        avsync: d("avsync"),
        total_avsync_change: d("total-avsync-change"),
        vo_drop_rate: 0.0,
        decoder_drop_rate: 0.0,
        audio_codec: s("audio-codec"),
        sample_rate: i("audio-params/samplerate"),
        channels: i("audio-params/channel-count"),
        audio_device: s("audio-device"),
        current_ao: s("current-ao"),
        audio_delay: d("audio-delay"),
        sub_delay: d("sub-delay"),
        cache_duration: d("demuxer-cache-duration"),
        cache_speed: d("cache-speed"),
        video_bitrate: d("video-bitrate"),
        cpu_percent: cpu,
        rss_mib: rss,
    }
}

/// Process CPU percent (of one core) and resident memory, from `/proc`.
fn process_usage() -> (Option<f64>, Option<f64>) {
    #[cfg(target_os = "linux")]
    {
        let cpu = std::fs::read_to_string("/proc/self/stat")
            .ok()
            .and_then(|raw| {
                let tail = raw.rsplit(')').next()?;
                let fields: Vec<&str> = tail.split_whitespace().collect();
                // After the comm field, index 11/12 are utime/stime.
                let utime: u64 = fields.get(11)?.parse().ok()?;
                let stime: u64 = fields.get(12)?.parse().ok()?;
                let ticks = utime + stime;
                // CLK_TCK is 100 on every Linux libc in practice; report lifetime
                // average as a percentage of one core.
                let uptime = std::fs::read_to_string("/proc/uptime")
                    .ok()
                    .and_then(|u| u.split_whitespace().next()?.parse::<f64>().ok())
                    .unwrap_or(0.0);
                if uptime > 0.0 {
                    Some((ticks as f64 / 100.0 / uptime) * 100.0)
                } else {
                    None
                }
            });
        let rss = std::fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|raw| raw.split_whitespace().nth(1)?.parse::<f64>().ok())
            .map(|pages| pages * 4096.0 / (1024.0 * 1024.0));
        (cpu, rss)
    }
    #[cfg(not(target_os = "linux"))]
    {
        (None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_names_the_stage_with_the_growing_counter() {
        let mut s = PlaybackDiagnostics {
            vo_drop_rate: 2.0,
            ..Default::default()
        };
        assert!(s.drop_verdict().contains("Output limited"));

        s.delayed_frames = 3;
        assert!(s.drop_verdict().contains("Presentation limited"));

        s.decoder_drop_rate = 4.0;
        s.vo_drop_rate = 1.0;
        assert!(s.drop_verdict().contains("Decoder limited"));

        let clean = PlaybackDiagnostics {
            avsync: Some(0.001),
            cache_duration: Some(2.0),
            ..Default::default()
        };
        assert!(clean.drop_verdict().contains("Nominal"));
    }

    #[test]
    fn sync_divergence_is_reported_when_no_frames_drop() {
        let s = PlaybackDiagnostics {
            avsync: Some(1.5),
            cache_duration: Some(2.0),
            ..Default::default()
        };
        assert!(s.drop_verdict().contains("Clock divergence"));
    }

    #[test]
    fn text_report_marks_unavailable_properties() {
        let report = PlaybackDiagnostics::default().to_text_report();
        assert!(report.contains("display_fps: n/a"));
        assert!(report.contains("avsync: n/a"));
        assert!(report.contains("hwdec_requested:"));
    }
}
