//! Auto-discovery for external sidecar `.lrc` files adjacent to media tracks.

use std::path::{Path, PathBuf};

/// Inspects the parent directory of `media_path` for a matching `.lrc` or `.LRC` file.
///
/// For example, given `/music/Artist/Track 01.flac`, looks for:
/// 1. `/music/Artist/Track 01.lrc`
/// 2. `/music/Artist/Track 01.LRC`
pub fn find_sidecar_lrc(media_path: &Path) -> Option<PathBuf> {
    let parent = media_path.parent()?;
    let stem = media_path.file_stem()?.to_str()?;

    let candidate_lower = parent.join(format!("{}.lrc", stem));
    if candidate_lower.is_file() {
        return Some(candidate_lower);
    }

    let candidate_upper = parent.join(format!("{}.LRC", stem));
    if candidate_upper.is_file() {
        return Some(candidate_upper);
    }

    None
}

/// Inspects the parent directory of `media_path` for a matching plain text `.txt` or `.TXT` file.
///
/// For example, given `/music/Artist/Track 01.flac`, looks for:
/// 1. `/music/Artist/Track 01.txt`
/// 2. `/music/Artist/Track 01.TXT`
pub fn find_sidecar_txt(media_path: &Path) -> Option<PathBuf> {
    let parent = media_path.parent()?;
    let stem = media_path.file_stem()?.to_str()?;

    let candidate_lower = parent.join(format!("{}.txt", stem));
    if candidate_lower.is_file() {
        return Some(candidate_lower);
    }

    let candidate_upper = parent.join(format!("{}.TXT", stem));
    if candidate_upper.is_file() {
        return Some(candidate_upper);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_find_sidecar_lrc() {
        let dir = tempdir().unwrap();
        let audio_file = dir.path().join("song.mp3");
        let lrc_file = dir.path().join("song.lrc");

        std::fs::write(&audio_file, b"fake audio bytes").unwrap();
        assert_eq!(find_sidecar_lrc(&audio_file), None);

        std::fs::write(&lrc_file, b"[00:01.00]Sidecar line").unwrap();
        assert_eq!(find_sidecar_lrc(&audio_file), Some(lrc_file));
    }

    #[test]
    fn test_find_sidecar_txt() {
        let dir = tempdir().unwrap();
        let audio_file = dir.path().join("song.flac");
        let txt_file = dir.path().join("song.txt");

        std::fs::write(&audio_file, b"fake audio bytes").unwrap();
        assert_eq!(find_sidecar_txt(&audio_file), None);

        std::fs::write(&txt_file, b"Unsynchronized plain text lyrics").unwrap();
        assert_eq!(find_sidecar_txt(&audio_file), Some(txt_file));
    }
}
