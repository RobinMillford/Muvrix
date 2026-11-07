//! Platform environment, distribution, and display server capability detection.

use std::fs;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::path::PathBuf;

/// Session windowing system.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SessionType {
    Wayland,
    X11,
    WindowsDesktop,
    Cocoa,
    Headless,
    Unknown,
}

impl SessionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Wayland => "Wayland",
            Self::X11 => "X11",
            Self::WindowsDesktop => "Windows Desktop (Win32)",
            Self::Cocoa => "macOS Cocoa",
            Self::Headless => "Headless / Non-graphical",
            Self::Unknown => "Unknown Session",
        }
    }
}

/// Linux distribution metadata parsed from `/etc/os-release`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistroInfo {
    pub id: String,
    pub name: String,
    pub version_id: String,
    pub pretty_name: String,
}

impl Default for DistroInfo {
    fn default() -> Self {
        Self {
            id: String::from("unknown"),
            name: String::from("Unknown OS"),
            version_id: String::new(),
            pretty_name: String::from("Unknown Operating System"),
        }
    }
}

/// Comprehensive platform and environment diagnostic data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformInfo {
    pub os: String,
    pub kernel_version: String,
    pub distro: DistroInfo,
    pub session_type: SessionType,
    pub drm_card: Option<PathBuf>,
    pub drm_render_node: Option<PathBuf>,
    pub drm_accessible: bool,
}

impl PlatformInfo {
    /// Detects the active platform capabilities at runtime without distribution assumptions.
    pub fn detect() -> Self {
        let os = std::env::consts::OS.to_string();
        let kernel_version = Self::detect_kernel_version();
        let distro = Self::parse_os_release();
        let session_type = Self::detect_session_type();
        let (drm_card, drm_render_node, drm_accessible) = Self::detect_drm_devices();

        Self {
            os,
            kernel_version,
            distro,
            session_type,
            drm_card,
            drm_render_node,
            drm_accessible,
        }
    }

    fn detect_kernel_version() -> String {
        #[cfg(target_os = "linux")]
        {
            if let Ok(version_str) = fs::read_to_string("/proc/sys/kernel/osrelease") {
                return version_str.trim().to_string();
            }
        }
        String::from("Unknown")
    }

    /// Parses standard `/etc/os-release` (and `/usr/lib/os-release` fallback).
    pub fn parse_os_release() -> DistroInfo {
        let candidates = ["/etc/os-release", "/usr/lib/os-release"];
        for path in &candidates {
            if let Ok(content) = fs::read_to_string(path) {
                return Self::parse_os_release_content(&content);
            }
        }
        DistroInfo::default()
    }

    pub fn parse_os_release_content(content: &str) -> DistroInfo {
        let mut distro = DistroInfo::default();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let key = k.trim();
                let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
                match key {
                    "ID" => distro.id = val,
                    "NAME" => distro.name = val,
                    "VERSION_ID" => distro.version_id = val,
                    "PRETTY_NAME" => distro.pretty_name = val,
                    _ => {}
                }
            }
        }
        distro
    }

    fn detect_session_type() -> SessionType {
        #[cfg(target_os = "windows")]
        return SessionType::WindowsDesktop;

        #[cfg(target_os = "macos")]
        return SessionType::Cocoa;

        #[cfg(target_os = "linux")]
        {
            if let Ok(st) = std::env::var("XDG_SESSION_TYPE") {
                match st.to_lowercase().as_str() {
                    "wayland" => return SessionType::Wayland,
                    "x11" => return SessionType::X11,
                    _ => {}
                }
            }
            if std::env::var("WAYLAND_DISPLAY").is_ok() {
                return SessionType::Wayland;
            }
            if std::env::var("DISPLAY").is_ok() {
                return SessionType::X11;
            }
            SessionType::Headless
        }

        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        SessionType::Unknown
    }

    fn detect_drm_devices() -> (Option<PathBuf>, Option<PathBuf>, bool) {
        #[cfg(target_os = "linux")]
        {
            let card = Path::new("/dev/dri/card1");
            let card_path = if card.exists() {
                Some(card.to_path_buf())
            } else if Path::new("/dev/dri/card0").exists() {
                Some(PathBuf::from("/dev/dri/card0"))
            } else {
                None
            };

            let render = Path::new("/dev/dri/renderD128");
            let (render_path, accessible) = if render.exists() {
                let accessible = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(render)
                    .is_ok();
                (Some(render.to_path_buf()), accessible)
            } else {
                (None, false)
            };

            (card_path, render_path, accessible)
        }

        #[cfg(not(target_os = "linux"))]
        (None, None, false)
    }
}
