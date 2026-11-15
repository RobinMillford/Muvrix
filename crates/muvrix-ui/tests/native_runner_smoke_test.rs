//! Native Linux runner end-to-end smoke test.
//!
//! This is intentionally opt-in: it needs a real graphical session, libmpv and
//! the local media corpus. It is not a headless unit test. Run with:
//!
//! ```text
//! cargo test -p muvrix-ui --test native_runner_smoke_test -- --ignored --nocapture
//! ```

use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const RUN_SECONDS: u64 = 4;
const WATCHDOG: Duration = Duration::from_secs(25);

fn fixture() -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/MuvrixTestMedia/phase-0.4/B-performance/h264-5min-sustained.mp4");
    path.is_file().then_some(path)
}

fn has_graphical_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
}

#[test]
#[ignore = "requires a Linux graphical session (Wayland/X11), libmpv and the local media corpus"]
fn native_runner_starts_renders_and_exits_cleanly() {
    if !has_graphical_session() {
        eprintln!("SKIP: no WAYLAND_DISPLAY or DISPLAY; native runner needs a graphical session");
        return;
    }
    let Some(fixture) = fixture() else {
        eprintln!(
            "SKIP: local fixture missing: tests/MuvrixTestMedia/phase-0.4/B-performance/h264-5min-sustained.mp4"
        );
        return;
    };

    let root = std::env::temp_dir().join(format!("muvrix-native-smoke-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create smoke temp dir");
    let telemetry = root.join("telemetry.csv");
    let trace = root.join("event-trace.log");
    let log_path = root.join("run.log");
    let log = File::create(&log_path).expect("create run log");

    let binary = env!("CARGO_BIN_EXE_muvrix-ui");
    let mut child = Command::new(binary)
        .arg(fixture)
        .env("MUVRIX_RUN_SECONDS", RUN_SECONDS.to_string())
        .env("MUVRIX_RUN_NAME", "native-runner-smoke")
        .env("MUVRIX_RUNNER_TELEMETRY", &telemetry)
        .env("MUVRIX_EVENT_TRACE", &trace)
        .env("MUVRIX_MPV_STATE_CSV", root.join("mpv-state.csv"))
        .env("MUVRIX_WINDOW_WIDTH", "960")
        .env("MUVRIX_WINDOW_HEIGHT", "540")
        .env("MUVRIX_TEST_FULLSCREEN", "1")
        .env("MUVRIX_NO_HDR", "1")
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
                    "native runner did not exit within {:?}\n--- run.log tail ---\n{}",
                    WATCHDOG,
                    tail(&text, 40)
                );
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };

    let text = fs::read_to_string(&log_path).unwrap_or_default();
    assert!(
        status.success(),
        "native runner exited with {status:?}\n--- run.log tail ---\n{}",
        tail(&text, 40)
    );
    assert!(
        !text.contains("panicked at"),
        "native runner panicked\n--- run.log tail ---\n{}",
        tail(&text, 40)
    );

    let summary = text
        .lines()
        .find(|line| line.contains("presentation summary"))
        .unwrap_or_else(|| {
            panic!(
                "no presentation summary in run.log\n--- run.log tail ---\n{}",
                tail(&text, 40)
            )
        });
    let swaps: u64 = summary
        .split("swaps=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("cannot parse swap count from: {summary}"));
    assert!(swaps > 0, "native runner performed no swaps: {summary}");
    assert!(
        summary.contains("failures=0"),
        "native runner reported swap failures: {summary}"
    );

    let rows = fs::read_to_string(&telemetry)
        .expect("telemetry file missing")
        .lines()
        .count();
    assert!(rows > 1, "telemetry has no data rows: {telemetry:?}");
    assert!(trace.is_file(), "event trace missing: {trace:?}");

    eprintln!(
        "native runner smoke: exit ok, swaps={swaps}, telemetry_rows={rows}, trace={trace:?}"
    );
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}
