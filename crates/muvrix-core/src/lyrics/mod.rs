//! Synchronized LRC lyrics data models, parser, and timeline synchronization.

pub mod detector;

pub use detector::{find_sidecar_lrc, find_sidecar_txt};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Source origin and synchronization tier for lyrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricSource {
    #[default]
    ExternalSynced,
    EmbeddedSynced,
    ExternalUnsynced,
    EmbeddedUnsynced,
}

impl LyricSource {
    pub fn is_embedded(&self) -> bool {
        matches!(self, Self::EmbeddedSynced | Self::EmbeddedUnsynced)
    }

    pub fn is_synchronized(&self) -> bool {
        matches!(self, Self::ExternalSynced | Self::EmbeddedSynced)
    }

    pub fn display_badge(&self) -> &'static str {
        match self {
            Self::ExternalSynced => "External LRC (Synchronized)",
            Self::EmbeddedSynced => "Embedded Lyrics (Synchronized)",
            Self::ExternalUnsynced => "External Text (Unsynchronized)",
            Self::EmbeddedUnsynced => "Embedded Lyrics (Unsynchronized)",
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum LrcParseError {
    #[error("Empty or invalid LRC content")]
    EmptyContent,
    #[error("Failed to parse timestamp: {0}")]
    InvalidTimestamp(String),
}

/// A single timestamped word or syllable within an Enhanced LRC line (`<mm:ss.xx>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricWord {
    /// Timestamp in milliseconds from media start.
    pub timestamp_ms: i64,
    /// Word or syllable text payload.
    pub text: String,
}

impl LyricWord {
    pub fn timestamp_seconds(&self) -> f64 {
        self.timestamp_ms as f64 / 1000.0
    }
}

/// A single timestamped lyric line with optional Enhanced LRC word-level timing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricLine {
    /// Timestamp in milliseconds from media start.
    pub timestamp_ms: i64,
    /// Lyric text payload (clean, without word tags).
    pub text: String,
    /// Optional word-level timestamp breakdown for karaoke tracking.
    pub words: Vec<LyricWord>,
}

impl LyricLine {
    pub fn new(timestamp_ms: i64, text: String) -> Self {
        Self {
            timestamp_ms,
            text,
            words: Vec::new(),
        }
    }

    pub fn timestamp_seconds(&self) -> f64 {
        self.timestamp_ms as f64 / 1000.0
    }

    /// Locates the active word index for a given playback timestamp in milliseconds.
    pub fn find_active_word_index(&self, current_time_ms: i64) -> Option<usize> {
        if self.words.is_empty() {
            return None;
        }
        let idx = self
            .words
            .partition_point(|w| w.timestamp_ms <= current_time_ms);
        if idx == 0 {
            None
        } else {
            Some(idx - 1)
        }
    }
}

/// Structured LRC file metadata and ordered timeline lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LrcFile {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub author: Option<String>,
    pub offset_ms: i64,
    pub lines: Vec<LyricLine>,
    #[serde(default = "default_true")]
    pub is_synchronized: bool,
    #[serde(default)]
    pub source: LyricSource,
}

impl Default for LrcFile {
    fn default() -> Self {
        Self {
            title: None,
            artist: None,
            album: None,
            author: None,
            offset_ms: 0,
            lines: Vec::new(),
            is_synchronized: true,
            source: LyricSource::ExternalSynced,
        }
    }
}

impl LrcFile {
    /// Loads and parses an LRC file from the filesystem.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, LrcParseError> {
        let content = std::fs::read_to_string(path).map_err(|_| LrcParseError::EmptyContent)?;
        Self::parse(&content)
    }

    /// Loads and automatically parses an LRC or plain text lyric file from the filesystem.
    pub fn from_file_auto(path: impl AsRef<std::path::Path>) -> Result<Self, LrcParseError> {
        let content = std::fs::read_to_string(path).map_err(|_| LrcParseError::EmptyContent)?;
        Self::parse_auto(&content, LyricSource::ExternalSynced)
    }

    /// Parses an LRC document string with support for standard timestamps,
    /// multiple timestamps per line, and ID metadata tags.
    pub fn parse(content: &str) -> Result<Self, LrcParseError> {
        let trimmed = content.trim_start_matches('\u{feff}').trim();
        if trimmed.is_empty() {
            return Err(LrcParseError::EmptyContent);
        }

        let mut title = None;
        let mut artist = None;
        let mut album = None;
        let mut author = None;
        let mut offset_ms: i64 = 0;
        let mut raw_lines = Vec::new();

        for line in trimmed.lines() {
            let l = line.trim();
            if l.is_empty() {
                continue;
            }

            // Check for metadata tags: [key:value]
            if let Some(tag_content) = l.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                if let Some((key, val)) = tag_content.split_once(':') {
                    match key.trim().to_lowercase().as_str() {
                        "ti" => {
                            title = Some(val.trim().to_string());
                            continue;
                        }
                        "ar" => {
                            artist = Some(val.trim().to_string());
                            continue;
                        }
                        "al" => {
                            album = Some(val.trim().to_string());
                            continue;
                        }
                        "by" => {
                            author = Some(val.trim().to_string());
                            continue;
                        }
                        "offset" => {
                            if let Ok(parsed_offset) = val.trim().parse::<i64>() {
                                offset_ms = parsed_offset;
                            }
                            continue;
                        }
                        _ => {}
                    }
                }
            }

            // Extract all timestamps from the line: [mm:ss.xx] or [mm:ss.xxx]
            let mut timestamps = Vec::new();
            let mut text_start = 0;

            let bytes = l.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'[' {
                    if let Some(close_idx) = l[i..].find(']') {
                        let full_close = i + close_idx;
                        let tag = &l[i + 1..full_close];
                        if let Some(ts) = parse_timestamp(tag) {
                            timestamps.push(ts);
                            i = full_close + 1;
                            text_start = i;
                            continue;
                        }
                    }
                }
                break;
            }

            let raw_body = l[text_start..].trim();
            let (clean_text, words) = parse_enhanced_words(raw_body);
            for ts in timestamps {
                raw_lines.push(LyricLine {
                    timestamp_ms: ts,
                    text: clean_text.clone(),
                    words: words.clone(),
                });
            }
        }

        // Apply global offset
        if offset_ms != 0 {
            for line in &mut raw_lines {
                line.timestamp_ms = (line.timestamp_ms + offset_ms).max(0);
                for word in &mut line.words {
                    word.timestamp_ms = (word.timestamp_ms + offset_ms).max(0);
                }
            }
        }

        // Sort chronologically
        raw_lines.sort_by_key(|l| l.timestamp_ms);

        Ok(Self {
            title,
            artist,
            album,
            author,
            offset_ms,
            lines: raw_lines,
            is_synchronized: true,
            source: LyricSource::ExternalSynced,
        })
    }

    /// Constructs an unsynchronized `LrcFile` from plain text lyrics,
    /// preserving Unicode, paragraphs, line breaks, and indentation.
    pub fn from_unsynced_text(content: &str, source: LyricSource) -> Result<Self, LrcParseError> {
        let trimmed = content.trim_start_matches('\u{feff}').trim();
        if trimmed.is_empty() {
            return Err(LrcParseError::EmptyContent);
        }

        let mut lines = Vec::new();
        for line in content.trim_start_matches('\u{feff}').lines() {
            lines.push(LyricLine {
                timestamp_ms: -1,
                text: line.trim_end().to_string(),
                words: Vec::new(),
            });
        }

        Ok(Self {
            title: None,
            artist: None,
            album: None,
            author: None,
            offset_ms: 0,
            lines,
            is_synchronized: false,
            source,
        })
    }

    /// Automatically detects whether `content` contains valid timestamps.
    /// If valid timestamps are found, parses as synchronized lyrics;
    /// otherwise parses as honest unsynchronized lyrics without fake timestamps.
    pub fn parse_auto(content: &str, source_hint: LyricSource) -> Result<Self, LrcParseError> {
        match Self::parse(content) {
            Ok(mut lrc) if !lrc.lines.is_empty() => {
                lrc.source = if source_hint.is_embedded() {
                    LyricSource::EmbeddedSynced
                } else {
                    LyricSource::ExternalSynced
                };
                lrc.is_synchronized = true;
                Ok(lrc)
            }
            _ => {
                let actual_source = if source_hint.is_embedded() {
                    LyricSource::EmbeddedUnsynced
                } else {
                    LyricSource::ExternalUnsynced
                };
                Self::from_unsynced_text(content, actual_source)
            }
        }
    }

    /// Locates the active lyric line index for a given playback timestamp in milliseconds.
    /// If the lyrics are unsynchronized, this returns `None` to prevent spurious highlights.
    pub fn find_active_index(&self, current_time_ms: i64) -> Option<usize> {
        if !self.is_synchronized || self.lines.is_empty() {
            return None;
        }

        // Binary partition: find first line where line.timestamp_ms > current_time_ms
        let idx = self
            .lines
            .partition_point(|line| line.timestamp_ms <= current_time_ms);

        if idx == 0 {
            // Playhead is before the very first lyric line
            None
        } else {
            // The active line is the one immediately preceding the next line
            Some(idx - 1)
        }
    }

    /// Returns a reference to the active lyric line at `current_time_ms`.
    pub fn active_line(&self, current_time_ms: i64) -> Option<&LyricLine> {
        self.find_active_index(current_time_ms)
            .and_then(|idx| self.lines.get(idx))
    }
}

/// Parses timestamp strings in `mm:ss`, `mm:ss.xx`, or `mm:ss.xxx` formats.
fn parse_timestamp(s: &str) -> Option<i64> {
    let (min_str, sec_part) = s.split_once(':')?;
    let mins = min_str.trim().parse::<i64>().ok()?;

    let (sec_str, ms_val) = if let Some((secs, sub)) = sec_part.split_once('.') {
        let sec = secs.trim().parse::<i64>().ok()?;
        let sub_trimmed = sub.trim();
        let ms = match sub_trimmed.len() {
            1 => sub_trimmed.parse::<i64>().ok()? * 100,
            2 => sub_trimmed.parse::<i64>().ok()? * 10,
            3 => sub_trimmed.parse::<i64>().ok()?,
            _ => sub_trimmed[..3].parse::<i64>().ok()?,
        };
        (sec, ms)
    } else {
        let sec = sec_part.trim().parse::<i64>().ok()?;
        (sec, 0)
    };

    Some(mins * 60_000 + sec_str * 1_000 + ms_val)
}

/// Parses Enhanced LRC word tags `<mm:ss.xx>` into clean text and ordered word-level timestamps.
fn parse_enhanced_words(raw_text: &str) -> (String, Vec<LyricWord>) {
    let mut clean_text = String::new();
    let mut words = Vec::new();

    let mut remaining = raw_text;
    while !remaining.is_empty() {
        if let Some(open_idx) = remaining.find('<') {
            // Text before the tag
            let prefix = &remaining[..open_idx];
            clean_text.push_str(prefix);

            if let Some(close_idx) = remaining[open_idx..].find('>') {
                let full_close = open_idx + close_idx;
                let tag = &remaining[open_idx + 1..full_close];
                let after_tag = &remaining[full_close + 1..];

                if let Some(ts) = parse_timestamp(tag) {
                    // The word text extends until the next '<' or end of string
                    let word_text = if let Some(next_tag) = after_tag.find('<') {
                        &after_tag[..next_tag]
                    } else {
                        after_tag
                    };
                    if !word_text.is_empty() {
                        words.push(LyricWord {
                            timestamp_ms: ts,
                            text: word_text.to_string(),
                        });
                        clean_text.push_str(word_text);
                    }
                    remaining = &after_tag[word_text.len()..];
                } else {
                    // Not a valid timestamp tag, preserve literal '<'
                    clean_text.push('<');
                    remaining = &remaining[open_idx + 1..];
                }
            } else {
                // No closing '>', take remaining as plain text
                clean_text.push_str(&remaining[open_idx..]);
                break;
            }
        } else {
            clean_text.push_str(remaining);
            break;
        }
    }

    if words.is_empty() {
        (raw_text.trim().to_string(), Vec::new())
    } else {
        (clean_text.trim().to_string(), words)
    }
}

/// Resolves lyrics according to strict 4-level precedence:
/// 1. External synchronized LRC (from adjacent `.lrc`)
/// 2. Embedded synchronized lyrics (from ID3v2/Vorbis/MP4 tags with timestamps)
/// 3. External unsynchronized text (from adjacent `.txt` or untimed `.lrc`)
/// 4. Embedded unsynchronized text (from ID3v2/Vorbis/MP4 tags without timestamps)
pub fn resolve_lyrics(
    media_path: &std::path::Path,
    embedded_lyrics_text: Option<&str>,
) -> Option<LrcFile> {
    // 1. Check external .lrc sidecar for synchronized lyrics
    let mut external_lrc_content: Option<String> = None;
    if let Some(lrc_path) = find_sidecar_lrc(media_path) {
        if let Ok(content) = std::fs::read_to_string(&lrc_path) {
            if let Ok(lrc) = LrcFile::parse(&content) {
                if !lrc.lines.is_empty() {
                    return Some(lrc); // Tier 1: External Synced LRC
                }
            }
            external_lrc_content = Some(content);
        }
    }

    // 2. Check embedded synchronized lyrics
    if let Some(embedded) = embedded_lyrics_text {
        let trimmed = embedded.trim_start_matches('\u{feff}').trim();
        if !trimmed.is_empty() {
            if let Ok(mut lrc) = LrcFile::parse(trimmed) {
                if !lrc.lines.is_empty() {
                    lrc.source = LyricSource::EmbeddedSynced;
                    lrc.is_synchronized = true;
                    return Some(lrc); // Tier 2: Embedded Synced Lyrics
                }
            }
        }
    }

    // 3. Check external unsynchronized text (from untimed .lrc or adjacent .txt)
    if let Some(content) = external_lrc_content {
        if let Ok(lrc) = LrcFile::from_unsynced_text(&content, LyricSource::ExternalUnsynced) {
            if !lrc.lines.is_empty() {
                return Some(lrc); // Tier 3: External Unsynced (from .lrc)
            }
        }
    }
    if let Some(txt_path) = find_sidecar_txt(media_path) {
        if let Ok(content) = std::fs::read_to_string(&txt_path) {
            if let Ok(lrc) = LrcFile::from_unsynced_text(&content, LyricSource::ExternalUnsynced) {
                if !lrc.lines.is_empty() {
                    return Some(lrc); // Tier 3: External Unsynced (from .txt)
                }
            }
        }
    }

    // 4. Check embedded unsynchronized lyrics
    if let Some(embedded) = embedded_lyrics_text {
        let trimmed = embedded.trim_start_matches('\u{feff}').trim();
        if !trimmed.is_empty() {
            if let Ok(lrc) = LrcFile::from_unsynced_text(trimmed, LyricSource::EmbeddedUnsynced) {
                if !lrc.lines.is_empty() {
                    return Some(lrc); // Tier 4: Embedded Unsynced Lyrics
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_standard_and_multi_timestamp_lrc() {
        let lrc = r#"
[ti:Synthetic Symphony]
[ar:Muvrix Band]
[al:Test Suite Audio]
[offset:100]
[00:05.00]Instrumental Intro
[00:15.50][00:45.00]Chorus: Streaming high and low
[00:25.250]Verse 1: Packets flowing free
"#;

        let parsed = LrcFile::parse(lrc).expect("Parse LRC");
        assert_eq!(parsed.title.as_deref(), Some("Synthetic Symphony"));
        assert_eq!(parsed.artist.as_deref(), Some("Muvrix Band"));
        assert_eq!(parsed.album.as_deref(), Some("Test Suite Audio"));
        assert_eq!(parsed.offset_ms, 100);

        // Lines must be sorted:
        // 00:05.00 + 100ms = 5100ms
        // 00:15.50 + 100ms = 15600ms
        // 00:25.250 + 100ms = 25350ms
        // 00:45.00 + 100ms = 45100ms
        assert_eq!(parsed.lines.len(), 4);
        assert_eq!(parsed.lines[0].timestamp_ms, 5100);
        assert_eq!(parsed.lines[0].text, "Instrumental Intro");

        assert_eq!(parsed.lines[1].timestamp_ms, 15600);
        assert_eq!(parsed.lines[1].text, "Chorus: Streaming high and low");

        assert_eq!(parsed.lines[2].timestamp_ms, 25350);
        assert_eq!(parsed.lines[2].text, "Verse 1: Packets flowing free");

        assert_eq!(parsed.lines[3].timestamp_ms, 45100);
        assert_eq!(parsed.lines[3].text, "Chorus: Streaming high and low");
    }

    #[test]
    fn test_lyrics_timeline_synchronization() {
        let lrc = r#"
[00:02.00]First line
[00:08.00]Second line
[00:15.00]Third line
"#;
        let file = LrcFile::parse(lrc).unwrap();

        // Before first line
        assert_eq!(file.find_active_index(1000), None);

        // Exactly on first line
        assert_eq!(file.find_active_index(2000), Some(0));
        assert_eq!(file.active_line(2000).unwrap().text, "First line");

        // Midway between line 1 and 2
        assert_eq!(file.find_active_index(5000), Some(0));

        // Line 2
        assert_eq!(file.find_active_index(8000), Some(1));
        assert_eq!(file.active_line(8000).unwrap().text, "Second line");

        // Line 3 and beyond
        assert_eq!(file.find_active_index(20000), Some(2));
        assert_eq!(file.active_line(20000).unwrap().text, "Third line");
    }

    #[test]
    fn test_enhanced_lrc_word_level_timestamps() {
        let lrc = r#"
[ti:Enhanced Test]
[00:01.00]<00:01.00>Never <00:01.50>gonna <00:02.00>give <00:02.50>you <00:03.00>up
[00:04.00]<00:04.00>Never <00:04.50>gonna <00:05.00>let <00:05.50>you <00:06.00>down
"#;
        let file = LrcFile::parse(lrc).expect("Parse Enhanced LRC");
        assert_eq!(file.lines.len(), 2);

        let line0 = &file.lines[0];
        assert_eq!(line0.timestamp_ms, 1000);
        assert_eq!(line0.text, "Never gonna give you up");
        assert_eq!(line0.words.len(), 5);
        assert_eq!(line0.words[0].text, "Never ");
        assert_eq!(line0.words[0].timestamp_ms, 1000);
        assert_eq!(line0.words[1].text, "gonna ");
        assert_eq!(line0.words[1].timestamp_ms, 1500);
        assert_eq!(line0.words[2].text, "give ");
        assert_eq!(line0.words[2].timestamp_ms, 2000);
        assert_eq!(line0.words[3].text, "you ");
        assert_eq!(line0.words[3].timestamp_ms, 2500);
        assert_eq!(line0.words[4].text, "up");
        assert_eq!(line0.words[4].timestamp_ms, 3000);

        // Word timeline tracking
        assert_eq!(line0.find_active_word_index(500), None);
        assert_eq!(line0.find_active_word_index(1000), Some(0)); // "Never "
        assert_eq!(line0.find_active_word_index(1700), Some(1)); // "gonna "
        assert_eq!(line0.find_active_word_index(2200), Some(2)); // "give "
        assert_eq!(line0.find_active_word_index(2800), Some(3)); // "you "
        assert_eq!(line0.find_active_word_index(3500), Some(4)); // "up"
    }
}
