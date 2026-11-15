//! Hardware regression for per-media decoder policy scope (D4).
//!
//! The test runs the real native Linux runner, forcing the first SDR H.264 item
//! through the same automatic fallback used by production. It then loads HDR
//! HEVC twice and verifies each HDR item starts from `auto-safe` and settles on
//! direct VAAPI (`hwdec-current=vaapi`, pixel format `vaapi`, DirectHw).
//!
//! This is ignored because it needs a Wayland session, a real VAAPI decoder and
//! locally cached fixtures. It is not a substitute for the unit tests that prove
//! `DecodePolicy::initial_mode()` never returns a transient fallback.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const RUN_SECONDS: u64 = 32;
const WATCHDOG: Duration = Duration::from_secs(75);

fn fixture(relative: &str) -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(relative);
    path.is_file().then(|| path.canonicalize().unwrap_or(path))
}

fn has_graphical_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

fn observation_for<'a>(log: &'a str, path: &Path) -> Vec<&'a str> {
    let needle = format!("Decoder observed: file='{}'", path.display());
    log.lines().filter(|line| line.contains(&needle)).collect()
}

#[test]
#[ignore = "requires a graphical Wayland session, direct VAAPI and the local SDR/HDR fixtures"]
fn sequential_sdr_hdr_loads_restore_direct_vaapi() {
    if !has_graphical_session() {
        eprintln!("SKIP: no WAYLAND_DISPLAY or DISPLAY");
        return;
    }
    let sdr = PathBuf::from("/tmp/sdr-bt709-pattern.mp4")
        .is_file()
        .then(|| PathBuf::from("/tmp/sdr-bt709-pattern.mp4"))
        .or_else(|| {
            fixture("tests/MuvrixTestMedia/phase-0.4/B-performance/h264-5min-sustained.mp4")
        })
        .or_else(|| fixture("tests/fixtures/test_video.mp4"));
    let Some(sdr) = sdr else {
        eprintln!("SKIP: SDR H.264 fixture missing");
        return;
    };
    let Some(hdr) = fixture("tests/fixtures/hdr10_pq_bt2020.mp4").or_else(|| {
        fixture("tests/MuvrixTestMedia/phase-0.4/generated/hevc-main10-hdr10-synthetic.mp4")
    }) else {
        eprintln!("SKIP: HDR HEVC fixture missing");
        return;
    };

    let root = std::env::temp_dir().join(format!("muvrix-decoder-sequence-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create decoder-sequence temp dir");
    let log_path = root.join("run.log");
    let log = fs::File::create(&log_path).expect("create run log");

    let sequence = format!(
        "0:{}|8:{}|16:{}|24:{}",
        sdr.display(),
        hdr.display(),
        sdr.display(),
        hdr.display()
    );
    let binary = env!("CARGO_BIN_EXE_muvrix-ui");
    let mut child = Command::new(binary)
        .env("MUVRIX_RUN_SECONDS", RUN_SECONDS.to_string())
        .env("MUVRIX_RUN_NAME", "decoder-policy-sequence")
        .env("MUVRIX_NO_HDR", "1")
        .env("MUVRIX_WINDOW_WIDTH", "960")
        .env("MUVRIX_WINDOW_HEIGHT", "540")
        .env("MUVRIX_TEST_FULLSCREEN", "1")
        .env("MUVRIX_TEST_SEQUENCE", sequence)
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .spawn()
        .expect("spawn native runner");

    let started = Instant::now();
    let status = loop {
        match child.try_wait().expect("poll native runner") {
            Some(status) => break status,
            None if started.elapsed() > WATCHDOG => {
                let _ = child.kill();
                let _ = child.wait();
                let text = fs::read_to_string(&log_path).unwrap_or_default();
                panic!(
                    "decoder sequence did not exit within {WATCHDOG:?}\n--- run.log tail ---\n{}",
                    tail(&text, 60)
                );
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    };

    let text = fs::read_to_string(&log_path).expect("read decoder sequence log");
    assert!(
        status.success(),
        "native runner exited with {status:?}\n--- run.log tail ---\n{}",
        tail(&text, 60)
    );
    assert!(
        !text.contains("panicked at"),
        "native runner panicked\n--- run.log tail ---\n{}",
        tail(&text, 60)
    );

    let hdr_observations = observation_for(&text, &hdr);
    assert!(
        hdr_observations.len() >= 2,
        "expected observations for both HDR loads, got {}\n--- run.log tail ---\n{}",
        hdr_observations.len(),
        tail(&text, 80)
    );
    for line in hdr_observations {
        assert!(
            line.contains("hwdec='vaapi'"),
            "HDR item did not settle on direct VAAPI: {line}"
        );
        assert!(
            line.contains("pixelformat='vaapi'"),
            "HDR item did not retain a hardware surface: {line}"
        );
        assert!(
            line.contains("path=DirectHw"),
            "HDR item was not on the direct path: {line}"
        );
        assert!(
            line.contains("requested='auto-safe'"),
            "HDR item did not start from the automatic policy top: {line}"
        );
    }
}
