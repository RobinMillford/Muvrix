//! Sandboxed `mpv.conf` / `input.conf` read-write helpers.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use muvrix_player::PersistedMediaSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfigEditorFile {
    #[default]
    MpvConf,
    InputConf,
}

impl ConfigEditorFile {
    pub fn label(self) -> &'static str {
        match self {
            Self::MpvConf => "mpv.conf",
            Self::InputConf => "input.conf",
        }
    }

    pub fn file_name(self) -> &'static str {
        self.label()
    }
}

/// Muvrix's sandboxed configuration directory.
pub fn config_dir() -> PathBuf {
    PersistedMediaSettings::default_settings_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".config/muvrix"))
}

pub fn config_path(file: ConfigEditorFile) -> PathBuf {
    config_dir().join(file.file_name())
}

pub fn load(file: ConfigEditorFile) -> io::Result<String> {
    load_path(&config_path(file))
}

pub fn save(file: ConfigEditorFile, contents: &str) -> io::Result<PathBuf> {
    let path = config_path(file);
    save_path(&path, contents)?;
    Ok(path)
}

pub fn load_path(path: &Path) -> io::Result<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(path)
}

pub fn save_path(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("conf.tmp");
    fs::write(&temp, contents.as_bytes())?;
    fs::rename(temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_editor_roundtrips_atomically() {
        let path = std::env::temp_dir().join(format!(
            "muvrix-config-editor-{}-{}.conf",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        save_path(&path, "vo=gpu\nhwdec=auto-safe\n").expect("save");
        assert_eq!(load_path(&path).expect("load"), "vo=gpu\nhwdec=auto-safe\n");
        save_path(&path, "volume=80\n").expect("overwrite");
        assert_eq!(load_path(&path).expect("reload"), "volume=80\n");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn missing_config_reads_as_empty() {
        let path = std::env::temp_dir().join(format!(
            "muvrix-missing-config-{}-{}.conf",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        assert_eq!(load_path(&path).expect("missing"), "");
    }
}
