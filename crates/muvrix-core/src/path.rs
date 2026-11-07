//! Media path resolution, home expansion, and project test fixture discovery.

use std::path::{Path, PathBuf};

/// Error encountered during media path resolution.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MediaPathError {
    /// The specified file does not exist at any candidate location.
    #[error("Media file not found: '{attempted}'. Searched candidate locations: {candidates:?}")]
    FileNotFound {
        attempted: String,
        candidates: Vec<String>,
    },

    /// The specified file exists but cannot be accessed or read.
    #[error("Media path is unreadable (permission denied): '{path}'")]
    PermissionDenied { path: String },

    /// The input string was empty or contained only whitespace.
    #[error("Media path is empty")]
    EmptyPath,
}

/// Expands leading `~` or `~/` to the current user's home directory.
///
/// Uses standard environment variables `$HOME` (Unix) and `%USERPROFILE%` (Windows)
/// without hardcoding any user-specific path.
pub fn expand_tilde<P: AsRef<Path>>(path: P) -> PathBuf {
    let p = path.as_ref();
    let path_str = p.to_string_lossy();

    if path_str == "~" || path_str.starts_with("~/") || path_str.starts_with("~\\") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            let home_buf = PathBuf::from(home);
            if path_str == "~" {
                return home_buf;
            }
            let sub = &path_str[2..];
            return home_buf.join(sub);
        }
    }
    p.to_path_buf()
}

/// Determines whether an input path string represents a streaming URL protocol.
pub fn is_streaming_url(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("rtmp://")
        || lower.starts_with("rtmps://")
        || lower.starts_with("rtsp://")
        || lower.starts_with("rtsps://")
        || lower.starts_with("mms://")
        || lower.starts_with("mmsh://")
        || lower.starts_with("hls://")
        || lower.starts_with("udp://")
        || lower.starts_with("tcp://")
        || lower.starts_with("edl://")
        || lower.starts_with("fd://")
        || lower.starts_with("ytdl://")
}

/// Discovers the Muvrix repository or workspace root directory by walking up from
/// the current working directory, the executable location, or cargo environment variables.
pub fn find_project_root() -> Option<PathBuf> {
    // 1. Check explicit environment override
    if let Some(root) = std::env::var_os("MUVRIX_ROOT") {
        let p = PathBuf::from(root);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. Check cargo manifest directory if built/run in cargo context
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let mut cur = PathBuf::from(manifest_dir);
        for _ in 0..5 {
            if is_project_root(&cur) {
                return Some(cur);
            }
            if !cur.pop() {
                break;
            }
        }
    }

    // 3. Walk upwards from current working directory
    if let Ok(cwd) = std::env::current_dir() {
        let mut cur = cwd;
        for _ in 0..5 {
            if is_project_root(&cur) {
                return Some(cur);
            }
            if !cur.pop() {
                break;
            }
        }
    }

    // 4. Walk upwards from executable path
    if let Ok(exe) = std::env::current_exe() {
        if let Some(mut cur) = exe.parent().map(|p| p.to_path_buf()) {
            for _ in 0..6 {
                if is_project_root(&cur) {
                    return Some(cur);
                }
                if !cur.pop() {
                    break;
                }
            }
        }
    }

    None
}

/// Validates whether a directory looks like the Muvrix project root.
fn is_project_root(path: &Path) -> bool {
    // Check for Muvrix workspace Cargo.toml
    let cargo_toml = path.join("Cargo.toml");
    if cargo_toml.is_file() {
        if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
            if content.contains("[workspace]") && content.contains("crates/muvrix-core") {
                return true;
            }
        }
    }

    // Check for tests/MuvrixTestMedia presence
    if path.join("tests").join("MuvrixTestMedia").exists() {
        return true;
    }

    false
}

/// Resolves an input path string (which may be a URL, an absolute path, a tilde-prefixed path,
/// or a relative path from arbitrary working directories) into an accessible, canonicalized `PathBuf`.
///
/// # Search Strategy for Relative Paths
/// If the path is relative, it checks:
/// 1. The current working directory (`CWD`)
/// 2. The detected project root (`find_project_root()`)
/// 3. Subdirectories of the project root: `tests/MuvrixTestMedia/phase-0.4/`, `tests/MuvrixTestMedia/`
/// 4. The user's home video directory: `$HOME/Videos/MuvrixTestMedia/phase-0.4/`, `$HOME/Videos/`
/// 5. The executable parent directory
pub fn resolve_media_path(raw_input: &str) -> Result<PathBuf, MediaPathError> {
    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return Err(MediaPathError::EmptyPath);
    }

    // Streaming URLs pass through untouched
    if is_streaming_url(trimmed) {
        return Ok(PathBuf::from(trimmed));
    }

    // Expand ~ / ~/
    let expanded = expand_tilde(trimmed);

    // If absolute path
    if expanded.is_absolute() {
        if expanded.exists() {
            return Ok(expanded.canonicalize().unwrap_or(expanded));
        } else {
            return Err(MediaPathError::FileNotFound {
                attempted: raw_input.to_string(),
                candidates: vec![expanded.display().to_string()],
            });
        }
    }

    // Build candidate search directories
    let mut candidate_roots = Vec::new();

    // 1. Current working directory
    if let Ok(cwd) = std::env::current_dir() {
        candidate_roots.push(cwd);
    }

    // 2. Project root and known media subdirectories
    if let Some(proj_root) = find_project_root() {
        candidate_roots.push(proj_root.clone());
        candidate_roots.push(
            proj_root
                .join("tests")
                .join("MuvrixTestMedia")
                .join("phase-0.4"),
        );
        candidate_roots.push(proj_root.join("tests").join("MuvrixTestMedia"));
        candidate_roots.push(
            proj_root
                .join("test")
                .join("MuvrixTestMedia")
                .join("phase-0.4"),
        );
        candidate_roots.push(proj_root.join("test").join("MuvrixTestMedia"));
    }

    // 3. Executable parent directory
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidate_roots.push(parent.to_path_buf());
        }
    }

    // 4. User Videos directory
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home_videos = PathBuf::from(home).join("Videos");
        candidate_roots.push(home_videos.join("MuvrixTestMedia").join("phase-0.4"));
        candidate_roots.push(home_videos.join("MuvrixTestMedia"));
        candidate_roots.push(home_videos);
    }

    let mut attempted_paths = Vec::new();

    for root in candidate_roots {
        let candidate = root.join(&expanded);
        let candidate_str = candidate.display().to_string();

        if attempted_paths.contains(&candidate_str) {
            continue;
        }

        if candidate.exists() {
            return Ok(candidate.canonicalize().unwrap_or(candidate));
        }

        attempted_paths.push(candidate_str);
    }

    Err(MediaPathError::FileNotFound {
        attempted: raw_input.to_string(),
        candidates: attempted_paths,
    })
}

/// Resolves the first existing media path from an ordered list of candidate paths.
///
/// This allows UI test presets to specify preferred modern Phase 0.4 test fixtures
/// while gracefully falling back to baseline fixtures or external user test media.
pub fn resolve_preset(candidates: &[&str]) -> Result<PathBuf, MediaPathError> {
    let mut all_attempted = Vec::new();

    for &cand in candidates {
        match resolve_media_path(cand) {
            Ok(resolved) => return Ok(resolved),
            Err(MediaPathError::FileNotFound {
                candidates: attempted,
                ..
            }) => {
                all_attempted.extend(attempted);
            }
            Err(e) => return Err(e),
        }
    }

    Err(MediaPathError::FileNotFound {
        attempted: candidates.join(" | "),
        candidates: all_attempted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tilde_expansion() {
        if let Some(home) = std::env::var_os("HOME") {
            let home_buf = PathBuf::from(home);
            assert_eq!(expand_tilde("~"), home_buf);
            assert_eq!(
                expand_tilde("~/Videos/sample.mp4"),
                home_buf.join("Videos/sample.mp4")
            );
        }
        assert_eq!(
            expand_tilde("/var/log/test"),
            PathBuf::from("/var/log/test")
        );
        assert_eq!(
            expand_tilde("relative/path.mp4"),
            PathBuf::from("relative/path.mp4")
        );
    }

    #[test]
    fn test_streaming_urls() {
        assert!(is_streaming_url("https://example.com/live/stream.m3u8"));
        assert!(is_streaming_url("http://127.0.0.1:8080/stream"));
        assert!(is_streaming_url("rtmp://live.twitch.tv/app"));
        assert!(is_streaming_url("rtsp://cam.local/h264"));
        assert!(!is_streaming_url("/tmp/video.mp4"));
        assert!(!is_streaming_url("tests/baseline.mp4"));
    }

    #[test]
    fn test_resolve_empty_and_url() {
        assert_eq!(resolve_media_path(""), Err(MediaPathError::EmptyPath));
        assert_eq!(resolve_media_path("   "), Err(MediaPathError::EmptyPath));

        let url = "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8";
        assert_eq!(resolve_media_path(url).unwrap(), PathBuf::from(url));
    }

    #[test]
    fn test_resolve_project_relative_fixture() {
        let resolved =
            resolve_media_path("tests/fixtures/path-resolution/project-relative-fixture.txt");
        assert!(
            resolved.is_ok(),
            "Expected project-relative fixture to resolve: {:?}",
            resolved
        );
        assert!(resolved.unwrap().exists());
    }

    #[test]
    fn test_resolve_preset_fallback() {
        let candidates = [
            "tests/fixtures/path-resolution/non_existent_fixture_9999.txt",
            "tests/fixtures/path-resolution/preset-fallback-fixture.txt",
        ];
        let resolved = resolve_preset(&candidates);
        assert!(
            resolved.is_ok(),
            "Expected preset fallback to succeed: {:?}",
            resolved
        );
        assert!(resolved.unwrap().exists());
    }

    #[test]
    fn test_resolve_spaces_in_filename() {
        let space_file = "tests/fixtures/path-resolution/fixture with spaces.txt";
        let resolved = resolve_media_path(space_file);
        assert!(
            resolved.is_ok(),
            "Expected spaced filename to resolve: {:?}",
            resolved
        );
        assert!(resolved.unwrap().exists());
    }

    #[test]
    fn test_resolve_nonexistent_returns_clear_error() {
        let res = resolve_media_path("some/completely/fake/video_file_xyz_123.mp4");
        assert!(matches!(res, Err(MediaPathError::FileNotFound { .. })));
    }
}
