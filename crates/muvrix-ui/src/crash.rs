//! Structured panic reports in Muvrix's local data directory.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CRASH_EXTENSION: &str = "crash";

pub fn data_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg).join("muvrix");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("muvrix")
    } else {
        std::env::temp_dir().join("muvrix")
    }
}

pub fn crashes_dir() -> PathBuf {
    data_dir().join("crashes")
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Install the global hook while preserving the default stderr printer.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".to_string());
        let location = info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        if let Err(error) = write_report_in(
            &crashes_dir(),
            &message,
            &location,
            &backtrace,
            &format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        ) {
            eprintln!("Muvrix: failed to write crash report: {error}");
        }
        default_hook(info);
    }));
}

pub fn write_report_in(
    directory: &Path,
    message: &str,
    location: &str,
    backtrace: &str,
    platform: &str,
) -> io::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let seconds = timestamp();
    let path = directory.join(format!(
        "crash-{seconds}-{}.{}",
        std::process::id(),
        CRASH_EXTENSION
    ));
    let body = format!(
        "timestamp={seconds}\nversion={}\nplatform={platform}\nlocation={location}\nmessage={message}\n\nbacktrace:\n{backtrace}\n",
        env!("CARGO_PKG_VERSION")
    );
    let temp = path.with_extension("crash.tmp");
    fs::write(&temp, body.as_bytes())?;
    fs::rename(temp, &path)?;
    Ok(path)
}

pub fn read_report(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
}

/// Newest unacknowledged report, if one exists.
pub fn pending_report() -> Option<PathBuf> {
    pending_report_in(&crashes_dir())
}

pub fn pending_report_in(directory: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(directory).ok()?;
    let mut reports: Vec<PathBuf> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|ext| ext.to_str()) == Some(CRASH_EXTENSION)).then_some(path)
        })
        .collect();
    reports.sort();
    reports.pop()
}

pub fn acknowledge(path: &Path) -> io::Result<()> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_report_roundtrips_and_pending_dismissal_works() {
        let dir = std::env::temp_dir().join(format!(
            "muvrix-crash-test-{}-{}",
            std::process::id(),
            timestamp()
        ));
        let _ = fs::remove_dir_all(&dir);
        let path = write_report_in(
            &dir,
            "boom from test",
            "src/test.rs:1:1",
            "stack line",
            "linux x86_64",
        )
        .expect("write report");
        let body = read_report(&path).expect("read report");
        assert!(body.contains("boom from test"));
        assert!(body.contains("linux x86_64"));
        assert_eq!(pending_report_in(&dir), Some(path.clone()));
        acknowledge(&path).expect("acknowledge");
        assert_eq!(pending_report_in(&dir), None);
        let _ = fs::remove_dir_all(dir);
    }
}
